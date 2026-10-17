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

//! The line protocol spoken with `FSSDispatcher` (dlight.remote.impl).
//!
//! Request:  `<kind> <id> <len> <escaped-path>[ <len2> <escaped-path2>]\n`
//! where lengths are in UTF-8 characters of the *escaped* text and `len == 0`
//! means "up to the next blank".
//!
//! Response lines start with `<kind> <id> `; directory entries are
//! `<name_len> <name> <type> <size> <mtime_ms> <rwx> <dev> <ino> <link_len> <link>`,
//! the same text that is stored in the persistence cache files.

use crate::fsutil::{self, lossy, Meta};
use crate::report_error;
use crate::settings::Settings;

pub const REQ_LS: u8 = b'l';
pub const REQ_RECURSIVE_LS: u8 = b'r';
pub const REQ_STAT: u8 = b'S';
pub const REQ_LSTAT: u8 = b's';
pub const REQ_COPY: u8 = b'C';
pub const REQ_MOVE: u8 = b'm';
pub const REQ_QUIT: u8 = b'q';
pub const REQ_SLEEP: u8 = b'P';
pub const REQ_ADD_WATCH: u8 = b'W';
pub const REQ_REMOVE_WATCH: u8 = b'w';
pub const REQ_REFRESH: u8 = b'R';
pub const REQ_DELETE: u8 = b'd';
pub const REQ_DELETE_ON_DISCONNECT: u8 = b'D';
pub const REQ_SERVER_INFO: u8 = b'i';
pub const REQ_HELP: u8 = b'?';
pub const REQ_OPTION: u8 = b'o';

pub const RSP_LS: u8 = b'l';
pub const RSP_RECURSIVE_LS: u8 = b'r';
pub const RSP_ENTRY: u8 = b'e';
pub const RSP_END: u8 = b'x';
pub const RSP_CHANGE: u8 = b'c';
pub const RSP_ERROR: u8 = b'E';
pub const RSP_REFRESH: u8 = b'R';
pub const RSP_SERVER_INFO: u8 = b'i';

pub const REQUEST_KINDS: &[(u8, &str)] = &[
    (REQ_LS, "FS_REQ_LS"),
    (REQ_RECURSIVE_LS, "FS_REQ_RECURSIVE_LS"),
    (REQ_STAT, "FS_REQ_STAT"),
    (REQ_LSTAT, "FS_REQ_LSTAT"),
    (REQ_COPY, "FS_REQ_COPY"),
    (REQ_MOVE, "FS_REQ_MOVE"),
    (REQ_QUIT, "FS_REQ_QUIT"),
    (REQ_SLEEP, "FS_REQ_SLEEP"),
    (REQ_ADD_WATCH, "FS_REQ_ADD_WATCH"),
    (REQ_REMOVE_WATCH, "FS_REQ_REMOVE_WATCH"),
    (REQ_REFRESH, "FS_REQ_REFRESH"),
    (REQ_DELETE, "FS_REQ_DELETE"),
    (REQ_DELETE_ON_DISCONNECT, "FS_REQ_DELETE_ON_DISCONNECT"),
    (REQ_SERVER_INFO, "FS_REQ_SERVER_INFO"),
    (REQ_OPTION, "FS_REQ_OPTION"),
    (REQ_HELP, "FS_REQ_HELP"),
];

/// Same limit as the C server's PATH_MAX-based buffers.
const MAX_PATH_BYTES: usize = 4096 * 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub kind: u8,
    pub id: i32,
    pub path: Vec<u8>,
    pub path2: Option<Vec<u8>>,
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

/// Decimal field terminated by a blank or end of input; returns (value, offset after the
/// terminator). `maxlen` bounds the field width like the C DECLARE_DECODE macro.
fn decode_num(s: &[u8], pos: usize, maxlen: usize) -> Option<(u64, usize)> {
    let mut v: u64 = 0;
    let mut i = pos;
    match s.get(i) {
        Some(c) if c.is_ascii_digit() => {}
        c => {
            report_error!(
                "unexpected numeric value: '{}'\n",
                c.map_or('\0', |&c| c as char)
            );
            return None;
        }
    }
    while i - pos < maxlen {
        match s.get(i) {
            Some(&c) if c.is_ascii_digit() => {
                v = v.saturating_mul(10).saturating_add(u64::from(c - b'0'));
                i += 1;
            }
            None | Some(0) => return Some((v, i + 1)),
            Some(&c) if is_space(c) => return Some((v, i + 1)),
            Some(&c) => {
                report_error!("unexpected numeric value: '{}'\n", c as char);
                return None;
            }
        }
    }
    report_error!("numeric value too long: '{}'\n", lossy(&s[pos..]));
    None
}

fn decode_int(s: &[u8], pos: usize) -> Option<(i32, usize)> {
    decode_num(s, pos, 12).map(|(v, p)| (v.min(i32::MAX as u64) as i32, p))
}

fn word_len(s: &[u8]) -> usize {
    s.iter().take_while(|&&c| c != 0 && !is_space(c)).count()
}

/// Reads one length-prefixed path at `pos`; returns (unescaped path, offset after it).
fn decode_path(raw: &[u8], pos: usize) -> Option<(Vec<u8>, usize)> {
    let (char_len, p) = decode_int(raw, pos)?;
    let rest = raw.get(p..).unwrap_or(&[]);
    let mut char_len = char_len.max(0) as usize;
    if char_len == 0 {
        char_len = word_len(rest);
    }
    let byte_len = fsutil::utf8_bytes_count(rest, char_len);
    if byte_len > MAX_PATH_BYTES {
        report_error!("wrong (too long path) request: {}", lossy(raw));
        return None;
    }
    // the second path follows the *escaped* first one plus a blank
    // (the C server skipped the unescaped length, mis-parsing paths that contained escapes)
    Some((fsutil::unescape(&rest[..byte_len]), p + byte_len + 1))
}

/// Decodes one raw request line (including its trailing '\n', if any).
pub fn decode_request(raw: &[u8]) -> Option<Request> {
    let kind = *raw.first()?;
    if kind == REQ_QUIT || kind == REQ_HELP {
        return Some(Request {
            kind,
            id: 0,
            path: Vec::new(),
            path2: None,
        });
    }
    if kind == 0 || kind == b'\n' {
        report_error!("wrong (zero length) request\n");
        return None;
    }
    if !REQUEST_KINDS.iter().any(|(k, _)| *k == kind) {
        report_error!("wrong request kind: {}", lossy(raw));
        return None;
    }
    if raw.get(1) != Some(&b' ') {
        report_error!("wrong request, no space after request kind: {}", lossy(raw));
        return None;
    }
    if kind == REQ_SERVER_INFO {
        let id = decode_int(raw, 2).map_or(0, |(id, _)| id);
        return Some(Request {
            kind,
            id,
            path: Vec::new(),
            path2: None,
        });
    }
    let (id, p) = decode_int(raw, 2)?;
    // a zero path length with no word is an error for all kinds but quit (handled above)
    let (len, after_len) = decode_int(raw, p)?;
    if len == 0 && word_len(raw.get(after_len..).unwrap_or(&[])) == 0 {
        report_error!("wrong (zero path) request: {}", lossy(raw));
        return None;
    }
    let (path, p) = decode_path(raw, p)?;
    let path2 = if kind == REQ_COPY || kind == REQ_MOVE {
        Some(decode_path(raw, p)?.0)
    } else {
        None
    };
    Some(Request {
        kind,
        id,
        path,
        path2,
    })
}

/// One directory entry as compared by the refresh manager.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: Vec<u8>,
    pub file_type: u8,
    pub size: u64,
    pub mtime: i64,
    pub can_read: bool,
    pub can_write: bool,
    pub can_exec: bool,
    pub dev: u64,
    pub ino: u64,
    pub link: Vec<u8>,
}

impl Entry {
    /// Builds an entry the same way an `ls` response line is built.
    pub fn from_meta(
        name: &[u8],
        meta: &Meta,
        link: &[u8],
        abspath: &[u8],
        settings: &Settings,
    ) -> Entry {
        let (r, w, x) = if meta.is_link() {
            (true, true, true) // the permissions of symbolic links are never used
        } else {
            fsutil::get_access(abspath, meta, settings)
        };
        Entry {
            name: name.to_vec(),
            file_type: meta.file_type_char(),
            size: meta.size,
            mtime: meta.mtime_ms,
            can_read: r,
            can_write: w,
            can_exec: x,
            dev: meta.dev,
            ino: meta.ino,
            link: if meta.is_link() {
                link.to_vec()
            } else {
                Vec::new()
            },
        }
    }

    /// `name_len name type size mtime rwx dev ino link_len link\n`
    pub fn format(&self) -> Vec<u8> {
        let name = fsutil::escape(&self.name);
        let link = fsutil::escape(&self.link);
        let mut out = Vec::with_capacity(name.len() + link.len() + 80);
        out.extend_from_slice(format!("{} ", fsutil::utf8_strlen(&name)).as_bytes());
        out.extend_from_slice(&name);
        out.push(b' ');
        out.push(self.file_type);
        // mtime is printed unsigned, like the C "%llu"
        out.extend_from_slice(format!(" {} {} ", self.size, self.mtime as u64).as_bytes());
        out.extend_from_slice(&fsutil::rwx(self.can_read, self.can_write, self.can_exec));
        out.extend_from_slice(
            format!(" {} {} {} ", self.dev, self.ino, fsutil::utf8_strlen(&link)).as_bytes(),
        );
        out.extend_from_slice(&link);
        out.push(b'\n');
        out
    }

    /// Parses a line written by `format()` (persistence cache, version 2).
    pub fn parse(line: &[u8]) -> Option<Entry> {
        let fail = |what: &str| {
            report_error!("wrong entry format: {}: {}", what, lossy(line));
            None
        };
        let (name_chars, p) = decode_int(line, 0)?;
        let name_bytes =
            fsutil::utf8_bytes_count(&line[p.min(line.len())..], name_chars.max(0) as usize);
        if p + name_bytes >= line.len() {
            return fail("too long name");
        }
        let name = fsutil::unescape(&line[p..p + name_bytes]);
        let mut p = p + name_bytes + 1;
        let file_type = *line.get(p)?;
        p += 2;
        let (size, np) = decode_num(line, p, 20)?;
        let (mtime, np) = decode_num(line, np, 20)?;
        p = np;
        let flag = |c: Option<&u8>, yes: u8| match c {
            Some(&c) if c == yes => Some(true),
            Some(b'-') => Some(false),
            _ => None,
        };
        let (Some(can_read), Some(can_write), Some(can_exec)) = (
            flag(line.get(p), b'r'),
            flag(line.get(p + 1), b'w'),
            flag(line.get(p + 2), b'x'),
        ) else {
            return fail("wrong access flags");
        };
        p += 4;
        let (dev, np) = decode_num(line, p, 20)?;
        let (ino, np) = decode_num(line, np, 20)?;
        let (link_chars, np) = decode_int(line, np)?;
        let link = if link_chars > 0 {
            let rest = line.get(np..).unwrap_or(&[]);
            let link_bytes = fsutil::utf8_bytes_count(rest, link_chars as usize);
            if np + link_bytes >= line.len() {
                return fail("too long link name");
            }
            fsutil::unescape(&rest[..link_bytes])
        } else {
            Vec::new()
        };
        if name.len() > 255 {
            return fail("too long file name");
        }
        if link.len() > 4096 {
            return fail("too long link name");
        }
        Some(Entry {
            name,
            file_type,
            size,
            mtime: mtime as i64,
            can_read,
            can_write,
            can_exec,
            dev,
            ino,
            link,
        })
    }
}

/// Formats an `lstat`-style entry for `abspath` (links are not followed).
/// Error: (errno, message) as the C response_entry_create() reports it.
pub fn lstat_entry(
    abspath: &[u8],
    basename: &[u8],
    settings: &Settings,
) -> Result<Vec<u8>, (i32, String)> {
    match fsutil::lstat(abspath, Some(settings)) {
        Ok(meta) => {
            let link = if meta.is_link() {
                match fsutil::readlink(abspath) {
                    Ok(l) => l,
                    Err(err) => {
                        report_error!(
                            "error performing readlink for {}: {}\n",
                            lossy(abspath),
                            fsutil::strerror(err)
                        );
                        b"?".to_vec()
                    }
                }
            } else {
                Vec::new()
            };
            Ok(Entry::from_meta(basename, &meta, &link, abspath, settings).format())
        }
        Err(err) => Err((
            err,
            format!(
                "error getting lstat for '{}': {}",
                lossy(abspath),
                fsutil::strerror(err)
            ),
        )),
    }
}

/// `<kind> <id> <char_len> <path>\n` — the header/end/change line shape.
pub fn path_line(kind: u8, id: i32, path: &[u8]) -> Vec<u8> {
    let mut out = format!("{} {} {} ", kind as char, id, fsutil::utf8_strlen(path)).into_bytes();
    out.extend_from_slice(path);
    out.push(b'\n');
    out
}

/// `E <id> <errno> <msg>: <strerror>: <path>\n`
pub fn error_line(id: i32, path: &[u8], err: i32, msg: &str) -> Vec<u8> {
    let strerr = if err != 0 {
        fsutil::strerror(err)
    } else {
        String::new()
    };
    let mut out =
        format!("{} {} {} {}: {}: ", RSP_ERROR as char, id, err, msg, strerr).into_bytes();
    out.extend_from_slice(path);
    out.push(b'\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_basic() {
        let r = decode_request(b"l 12 4 /tmp\n").unwrap();
        assert_eq!(
            r,
            Request {
                kind: b'l',
                id: 12,
                path: b"/tmp".to_vec(),
                path2: None
            }
        );
    }

    #[test]
    fn decode_zero_len_takes_word() {
        let r = decode_request(b"S 3 0 /etc/passwd\n").unwrap();
        assert_eq!(r.path, b"/etc/passwd");
    }

    #[test]
    fn decode_utf8_and_spaces() {
        let line = "l 5 10 /tmp/a é b\n".as_bytes();
        assert_eq!(decode_request(line).unwrap().path, "/tmp/a é b".as_bytes());
    }

    #[test]
    fn decode_two_paths_with_escape() {
        // first path "/a\nb" escaped as "/a\\nb" (5 chars), second "/c"
        let r = decode_request(b"C 7 5 /a\\nb 2 /c\n").unwrap();
        assert_eq!(r.path, b"/a\nb");
        assert_eq!(r.path2.unwrap(), b"/c");
    }

    #[test]
    fn decode_info_and_quit() {
        assert_eq!(decode_request(b"i 1 0 \n").unwrap().id, 1);
        assert_eq!(decode_request(b"q 9 0 \n").unwrap().kind, REQ_QUIT);
    }

    #[test]
    fn decode_errors() {
        assert!(decode_request(b"\n").is_none());
        assert!(decode_request(b"Z 1 1 /\n").is_none());
        assert!(decode_request(b"l1 1 /\n").is_none());
        assert!(decode_request(b"l x 1 /\n").is_none());
        assert!(decode_request(b"l 1 0 \n").is_none());
    }

    #[test]
    fn entry_round_trip() {
        let e = Entry {
            name: "a b\\é".as_bytes().to_vec(),
            file_type: b'l',
            size: 12,
            mtime: 1_700_000_000_123,
            can_read: true,
            can_write: false,
            can_exec: true,
            dev: 2049,
            ino: 77,
            link: b"../x y".to_vec(),
        };
        let line = e.format();
        assert_eq!(Entry::parse(&line).unwrap(), e);
    }
}
