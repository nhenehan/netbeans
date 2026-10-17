// Licensed to the Apache Software Foundation (ASF) under one
// or more contributor license agreements.  See the NOTICE file
// distributed with this work for additional information
// regarding copyright ownership.  The ASF licenses this file
// to you under the Apache License, Version 2.0 (the
// "License"); you may not use this file except in compliance
// with the License.  You may obtain a copy of the License at
//
//   http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing,
// software distributed under the License is distributed on an
// "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied.  See the License for the
// specific language governing permissions and limitations
// under the License.

//! File system helpers: stat with the "forbidden" override, access checks,
//! directory walking, wire escaping and UTF-8 length arithmetic.
//!
//! Paths are raw bytes throughout: Unix file names need not be valid UTF-8.

use std::ffi::{CString, OsStr};
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::OnceLock;

use crate::report_error;
use crate::settings::Settings;

pub fn as_path(p: &[u8]) -> &Path {
    Path::new(OsStr::from_bytes(p))
}

pub fn lossy(p: &[u8]) -> std::borrow::Cow<'_, str> {
    String::from_utf8_lossy(p)
}

/// The subset of `struct stat` the protocol uses.
#[derive(Debug, Clone, Copy)]
pub struct Meta {
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    /// modification time in milliseconds since the epoch
    pub mtime_ms: i64,
    pub dev: u64,
    pub ino: u64,
}

// mode_t is u32 on Linux but u16 on macOS/BSD, so the casts are not redundant everywhere
#[allow(clippy::unnecessary_cast)]
impl Meta {
    fn from_std(m: &fs::Metadata) -> Meta {
        Meta {
            mode: m.mode(),
            uid: m.uid(),
            gid: m.gid(),
            size: m.size(),
            mtime_ms: m.mtime() * 1000 + m.mtime_nsec() / 1_000_000,
            dev: m.dev(),
            ino: m.ino(),
        }
    }

    /// What a forbidden directory reports: a world r-x directory with zero everything else.
    fn dummy() -> Meta {
        Meta {
            mode: libc::S_IFDIR as u32 | libc::S_IROTH as u32 | libc::S_IXOTH as u32,
            uid: 0,
            gid: 0,
            size: 0,
            mtime_ms: 0,
            dev: 0,
            ino: 0,
        }
    }

    fn fmt(&self) -> u32 {
        self.mode & libc::S_IFMT as u32
    }

    pub fn is_dir(&self) -> bool {
        self.fmt() == libc::S_IFDIR as u32
    }

    pub fn is_reg(&self) -> bool {
        self.fmt() == libc::S_IFREG as u32
    }

    pub fn is_link(&self) -> bool {
        self.fmt() == libc::S_IFLNK as u32
    }

    /// One-character file type as used on the wire (same letters as `ls -l`).
    pub fn file_type_char(&self) -> u8 {
        match self.fmt() {
            x if x == libc::S_IFIFO as u32 => b'p',
            x if x == libc::S_IFCHR as u32 => b'c',
            x if x == libc::S_IFDIR as u32 => b'd',
            x if x == libc::S_IFBLK as u32 => b'b',
            x if x == libc::S_IFREG as u32 => b'-',
            x if x == libc::S_IFLNK as u32 => b'l',
            x if x == libc::S_IFSOCK as u32 => b's',
            _ => b'u',
        }
    }
}

pub const FILETYPE_REG: u8 = b'-';
pub const FILETYPE_LNK: u8 = b'l';

fn errno_of(e: &io::Error) -> i32 {
    e.raw_os_error().unwrap_or(0)
}

/// lstat(2), unless `path` is forbidden to stat.
pub fn lstat(path: &[u8], settings: Option<&Settings>) -> Result<Meta, i32> {
    if settings.is_some_and(|s| s.is_forbidden(path)) {
        crate::trace!(
            crate::out::TRACE_FINEST,
            "Skipping forbidden lstat for {}\n",
            lossy(path)
        );
        return Ok(Meta::dummy());
    }
    fs::symlink_metadata(as_path(path))
        .map(|m| Meta::from_std(&m))
        .map_err(|e| errno_of(&e))
}

/// stat(2), unless `path` is forbidden to stat.
pub fn stat(path: &[u8], settings: Option<&Settings>) -> Result<Meta, i32> {
    if settings.is_some_and(|s| s.is_forbidden(path)) {
        crate::trace!(
            crate::out::TRACE_FINEST,
            "Skipping forbidden stat for {}\n",
            lossy(path)
        );
        return Ok(Meta::dummy());
    }
    fs::metadata(as_path(path))
        .map(|m| Meta::from_std(&m))
        .map_err(|e| errno_of(&e))
}

pub fn readlink(path: &[u8]) -> Result<Vec<u8>, i32> {
    fs::read_link(as_path(path))
        .map(|p| p.into_os_string().as_bytes().to_vec())
        .map_err(|e| errno_of(&e))
}

/// strerror(3) text for an errno value (thread-safe).
pub fn strerror(errno: i32) -> String {
    let mut buf = [0 as libc::c_char; 256];
    // SAFETY: buf is valid for buf.len() bytes; the XSI strerror_r NUL-terminates on success.
    let rc = unsafe { libc::strerror_r(errno, buf.as_mut_ptr(), buf.len()) };
    if rc != 0 {
        return format!("Unknown error {errno}");
    }
    // SAFETY: NUL-terminated by strerror_r.
    unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}

struct Identity {
    uid: libc::uid_t,
    groups: Vec<libc::gid_t>,
}

fn identity() -> &'static Identity {
    static ID: OnceLock<Identity> = OnceLock::new();
    ID.get_or_init(|| {
        // SAFETY: getuid has no preconditions; getgroups writes at most `groups.len()` entries.
        unsafe {
            let uid = libc::getuid();
            let mut groups = vec![0 as libc::gid_t; 256];
            let n = libc::getgroups(groups.len() as libc::c_int, groups.as_mut_ptr());
            groups.truncate(n.max(0) as usize);
            Identity { uid, groups }
        }
    })
}

/// Mode-bit check for owner / group / other, like the C `can()` (uses the real uid).
fn can(m: &Meta, other: u32, group: u32, user: u32) -> bool {
    let id = identity();
    let access = m.mode & 0o777;
    if m.uid == id.uid {
        access & user != 0
    } else if id.groups.contains(&m.gid) {
        access & group != 0
    } else {
        access & other != 0
    }
}

fn access(path: &[u8], mode: libc::c_int) -> bool {
    match CString::new(path) {
        // SAFETY: c is a valid NUL-terminated string.
        Ok(c) => unsafe { libc::access(c.as_ptr(), mode) == 0 },
        Err(_) => false,
    }
}

/// (can_read, can_write, can_exec) for the current user.
pub fn get_access(path: &[u8], m: &Meta, settings: &Settings) -> (bool, bool, bool) {
    if settings.full_access_check {
        (
            access(path, libc::R_OK),
            access(path, libc::W_OK),
            access(path, libc::X_OK),
        )
    } else {
        (
            can(m, 0o004, 0o040, 0o400),
            can(m, 0o002, 0o020, 0o200),
            can(m, 0o001, 0o010, 0o100),
        )
    }
}

pub fn rwx(r: bool, w: bool, x: bool) -> [u8; 3] {
    [
        if r { b'r' } else { b'-' },
        if w { b'w' } else { b'-' },
        if x { b'x' } else { b'-' },
    ]
}

/// Does not consult the forbidden list (same as the C dir_exists()).
pub fn dir_exists(path: &[u8]) -> bool {
    match fs::symlink_metadata(as_path(path)) {
        Ok(m) => m.file_type().is_dir(),
        Err(e) => !matches!(errno_of(&e), libc::ENOENT | libc::ENOTDIR | 0),
    }
}

pub fn file_exists(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Ok(_) => true,
        Err(e) => errno_of(&e) != libc::ENOENT,
    }
}

/// Entry callback: (name, meta, link target, absolute path) -> continue?
pub type Visitor<'a> = dyn FnMut(&[u8], &Meta, &[u8], &[u8]) -> bool + 'a;

/// Error callback: (dir_itself, path, errno, message).
pub type ErrorHandler<'a> = dyn FnMut(bool, &[u8], i32, &str) + 'a;

pub fn default_error_handler(_dir_itself: bool, path: &[u8], err: i32, msg: &str) {
    report_error!("{} '{}': {}\n", msg, lossy(path), strerror(err));
}

/// Calls `visitor(name, meta, link, abspath)` for each entry of `path` except "." and "..",
/// in readdir order. The visitor returns false to stop. Returns false if the directory
/// could not be opened.
pub fn visit_dir_entries(
    path: &[u8],
    settings: Option<&Settings>,
    on_error: &mut ErrorHandler<'_>,
    visitor: &mut Visitor<'_>,
) -> bool {
    let rd = match fs::read_dir(as_path(path)) {
        Ok(rd) => rd,
        Err(e) => {
            on_error(true, path, errno_of(&e), "error opening directory");
            return false;
        }
    };
    let base: &[u8] = if path == b"/" { b"" } else { path };
    for ent in rd {
        let ent = match ent {
            Ok(ent) => ent,
            Err(e) => {
                on_error(true, path, errno_of(&e), "error reading directory");
                break;
            }
        };
        let name_os = ent.file_name();
        let name = name_os.as_bytes();
        let mut abspath = Vec::with_capacity(base.len() + 1 + name.len());
        abspath.extend_from_slice(base);
        abspath.push(b'/');
        abspath.extend_from_slice(name);
        match lstat(&abspath, settings) {
            Ok(meta) => {
                let link = if meta.is_link() {
                    match readlink(&abspath) {
                        Ok(l) => l,
                        Err(err) => {
                            on_error(false, &abspath, err, "error performing readlink");
                            b"?".to_vec()
                        }
                    }
                } else {
                    Vec::new()
                };
                if !visitor(name, &meta, &link, &abspath) {
                    break;
                }
            }
            Err(err) => on_error(false, &abspath, err, "error getting stat"),
        }
    }
    true
}

/// Recursively removes the content of `path` (but not `path` itself).
pub fn clean_dir(path: &[u8]) -> bool {
    let mut ok = true;
    visit_dir_entries(
        path,
        None,
        &mut default_error_handler,
        &mut |_name, meta, _link, abspath| {
            if meta.is_dir() {
                if clean_dir(abspath) {
                    if let Err(e) = fs::remove_dir(as_path(abspath)) {
                        report_error!(
                            "error deleting '{}': {}\n",
                            lossy(abspath),
                            strerror(errno_of(&e))
                        );
                        ok = false;
                    }
                } else {
                    ok = false;
                }
            } else if let Err(e) = fs::remove_file(as_path(abspath)) {
                report_error!(
                    "error deleting '{}': {}\n",
                    lossy(abspath),
                    strerror(errno_of(&e))
                );
                ok = false;
            }
            true
        },
    );
    ok
}

/// Wire escaping: '\n' -> "\\n", '\\' -> "\\\\".
pub fn escape(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() + 8);
    for &c in s {
        match c {
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\\' => out.extend_from_slice(b"\\\\"),
            _ => out.push(c),
        }
    }
    out
}

pub fn unescape(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut esc = false;
    for &c in s {
        if esc {
            esc = false;
            match c {
                b'\\' => out.push(b'\\'),
                b'n' => out.push(b'\n'),
                _ => {
                    report_error!("wrong character '{}' in line {}\n", c as char, lossy(s));
                    out.push(c);
                }
            }
        } else if c == b'\\' {
            esc = true;
        } else {
            out.push(c);
        }
    }
    out
}

fn is_cont(b: u8) -> bool {
    (b & 0xC0) == 0x80
}

/// Number of UTF-8 code points in `s` (lengths on the wire are in characters).
pub fn utf8_strlen(s: &[u8]) -> usize {
    s.iter().filter(|&&b| !is_cont(b)).count()
}

/// Number of bytes that hold the first `char_count` characters of `s`
/// (stops at the end of `s` or at a NUL byte).
pub fn utf8_bytes_count(s: &[u8], char_count: usize) -> usize {
    let mut i = 0;
    let mut left = char_count;
    while i < s.len() && s[i] != 0 && left > 0 {
        if !is_cont(s[i]) {
            left -= 1;
        }
        i += 1;
    }
    while i < s.len() && s[i] != 0 && is_cont(s[i]) {
        i += 1;
    }
    i
}

pub fn basename(path: &[u8]) -> &[u8] {
    match path.iter().rposition(|&c| c == b'/') {
        Some(i) => &path[i + 1..],
        None => path,
    }
}

/// True if `child` is `parent` or lies below it (trailing slashes tolerated).
pub fn is_subdir(child: &[u8], parent: &[u8]) -> bool {
    let n = child.iter().zip(parent).take_while(|(a, b)| a == b).count();
    let (c, p) = (&child[n..], &parent[n..]);
    match (c.first(), p.first()) {
        (Some(_), Some(_)) => false,
        (None, None) => true,
        (None, Some(_)) => p.iter().all(|&b| b == b'/'),
        (Some(&first), None) => first == b'/' || (n > 0 && child[n - 1] == b'/'),
    }
}

/// Like the C is_prohibited(): pseudo file systems the refresh manager never polls.
pub fn is_prohibited(path: &[u8]) -> bool {
    matches!(path, b"/proc" | b"/dev") || (cfg!(target_os = "linux") && path == b"/run")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping_round_trip() {
        let s = b"a\\b\nc";
        assert_eq!(escape(s), b"a\\\\b\\nc");
        assert_eq!(unescape(&escape(s)), s);
    }

    #[test]
    fn utf8_counts() {
        let s = "aé€😀b".as_bytes();
        assert_eq!(utf8_strlen(s), 5);
        assert_eq!(utf8_bytes_count(s, 0), 0);
        assert_eq!(utf8_bytes_count(s, 2), 3);
        assert_eq!(utf8_bytes_count(s, 4), 10);
        assert_eq!(utf8_bytes_count(s, 99), s.len());
    }

    #[test]
    fn subdir() {
        assert!(is_subdir(b"/a/b", b"/a"));
        assert!(is_subdir(b"/a", b"/a"));
        assert!(is_subdir(b"/a", b"/a//"));
        assert!(is_subdir(b"/a/b", b"/a/"));
        assert!(is_subdir(b"/a/b", b"/"));
        assert!(!is_subdir(b"/ab", b"/a"));
        assert!(!is_subdir(b"/a", b"/a/b"));
    }

    #[test]
    fn base() {
        assert_eq!(basename(b"/a/b"), b"b");
        assert_eq!(basename(b"/"), b"");
        assert_eq!(basename(b"x"), b"x");
    }
}
