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

//! The directory table ("dirtab"): every directory the IDE has listed, with the
//! number of its cache file. Persisted as `<root>/dirtab`, one
//! `<index> <escaped-path>` line per directory; listings live in `<root>/cache/<index>`.
//!
//! The on-disk layout is identical to the C server's, so either implementation
//! can reuse the other's cache.

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use crate::exitcodes;
use crate::fsutil::{self, lossy};
use crate::{report_error, trace};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Initial,
    LsSent,
    RefreshSent,
    Removed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchState {
    None,
    Poll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshState {
    None,
    Refreshing,
    PendingRefresh,
}

#[derive(Debug)]
pub struct ElState {
    pub state: State,
    pub watch: WatchState,
    pub refresh: RefreshState,
}

#[derive(Debug)]
pub struct Element {
    pub index: usize,
    pub abspath: Vec<u8>,
    pub cache_path: PathBuf,
    st: Mutex<ElState>,
}

impl Element {
    pub fn lock(&self) -> MutexGuard<'_, ElState> {
        self.st.lock().unwrap_or_else(|p| p.into_inner())
    }
}

struct Table {
    dirty: bool,
    /// sorted by path (byte order) for binary search
    elems: Vec<Arc<Element>>,
    next_index: usize,
}

pub struct Dirtab {
    root: PathBuf,
    cache_dir: PathBuf,
    dirtab_file: PathBuf,
    table: Mutex<Table>,
}

static DIRTAB: OnceLock<Dirtab> = OnceLock::new();

pub fn get() -> &'static Dirtab {
    DIRTAB.get().expect("dirtab not initialized")
}

/// `<home>/.netbeans/remotefs[/<dir>]` unless `dir` is absolute; "0" is the default subdir.
pub fn resolve_root(persistence_dir: Option<&str>) -> PathBuf {
    let pdir = persistence_dir.unwrap_or("0");
    if pdir.starts_with('/') {
        return PathBuf::from(pdir);
    }
    match home_dir() {
        Some(home) => home.join(".netbeans").join("remotefs").join(pdir),
        None => {
            report_error!("can't determine home directory\n");
            process::exit(exitcodes::FAILURE_GETTING_HOME_DIR);
        }
    }
}

/// Home directory from the password database (like the C server), not from $HOME.
fn home_dir() -> Option<PathBuf> {
    use std::ffi::CStr;
    use std::os::unix::ffi::OsStrExt;
    // SAFETY: getpwuid_r writes only into the provided buffers; pwd.pw_dir points into buf.
    unsafe {
        let uid = libc::getuid();
        let mut buf = vec![0 as libc::c_char; 16 * 1024];
        let mut pwd: libc::passwd = std::mem::zeroed();
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        let rc = libc::getpwuid_r(uid, &mut pwd, buf.as_mut_ptr(), buf.len(), &mut result);
        if rc == 0 && !result.is_null() && !pwd.pw_dir.is_null() {
            let dir = CStr::from_ptr(pwd.pw_dir).to_bytes();
            return Some(PathBuf::from(std::ffi::OsStr::from_bytes(dir)));
        }
    }
    std::env::var_os("HOME").map(PathBuf::from)
}

fn mkdir_or_die(path: &Path, code_create: i32, code_access: i32) {
    match fs::metadata(path) {
        Ok(m) if m.is_dir() => {}
        Ok(_) => {
            report_error!(
                "error accessing directory '{}': not a directory\n",
                path.display()
            );
            process::exit(code_access);
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            use std::os::unix::fs::DirBuilderExt;
            if let Err(e) = fs::DirBuilder::new().mode(0o700).create(path) {
                report_error!(
                    "error creating directory '{}': {}\n",
                    path.display(),
                    fsutil::strerror(e.raw_os_error().unwrap_or(0))
                );
                process::exit(code_create);
            }
        }
        Err(e) => {
            report_error!(
                "error accessing directory '{}': {}\n",
                path.display(),
                fsutil::strerror(e.raw_os_error().unwrap_or(0))
            );
            process::exit(code_access);
        }
    }
}

fn mkdir_or_die_recursive(path: &Path, code_create: i32, code_access: i32) {
    let mut cur = PathBuf::new();
    for comp in path.components() {
        cur.push(comp);
        if cur.as_os_str().len() > 1 {
            mkdir_or_die(&cur, code_create, code_access);
        }
    }
}

/// Creates the persistence directories and loads `<root>/dirtab`.
pub fn init(root: PathBuf, clear_persistence: bool, default_watch: WatchState) {
    mkdir_or_die_recursive(
        &root,
        exitcodes::FAILURE_CREATING_STORAGE_DIR,
        exitcodes::FAILURE_ACCESSING_STORAGE_DIR,
    );
    if clear_persistence {
        trace!(
            crate::out::TRACE_INFO,
            "Cleaning up persistence ({})\n",
            root.display()
        );
        use std::os::unix::ffi::OsStrExt;
        fsutil::clean_dir(root.as_os_str().as_bytes());
    }
    let cache_dir = root.join("cache");
    mkdir_or_die(
        &cache_dir,
        exitcodes::FAILURE_CREATING_CACHE_DIR,
        exitcodes::FAILURE_ACCESSING_CACHE_DIR,
    );
    mkdir_or_die(
        &root.join("tmp"),
        exitcodes::FAILURE_CREATING_TEMP_DIR,
        exitcodes::FAILURE_ACCESSING_TEMP_DIR,
    );
    let dirtab_file = root.join("dirtab");
    let dt = Dirtab {
        root,
        cache_dir,
        dirtab_file,
        table: Mutex::new(Table {
            dirty: false,
            elems: Vec::new(),
            next_index: 0,
        }),
    };
    if DIRTAB.set(dt).is_err() {
        report_error!("directories table should only be initialized once!\n");
        process::exit(exitcodes::DIRTAB_DOUBLE_INITIALIZATION);
    }
    get().load(default_watch);
}

impl Dirtab {
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn table(&self) -> MutexGuard<'_, Table> {
        self.table.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn new_element(&self, path: &[u8], index: usize, watch: WatchState) -> Arc<Element> {
        Arc::new(Element {
            index,
            abspath: path.to_vec(),
            cache_path: self.cache_dir.join(index.to_string()),
            st: Mutex::new(ElState {
                state: State::Initial,
                watch,
                refresh: RefreshState::None,
            }),
        })
    }

    fn load(&self, default_watch: WatchState) {
        if !fsutil::file_exists(&self.dirtab_file) {
            return;
        }
        let f = match fs::File::open(&self.dirtab_file) {
            Ok(f) => f,
            Err(e) => {
                report_error!(
                    "error opening {}: {}\n",
                    self.dirtab_file.display(),
                    fsutil::strerror(e.raw_os_error().unwrap_or(0))
                );
                return;
            }
        };
        let mut t = self.table();
        let mut elems = Vec::new();
        let mut next_index = 0;
        for line in BufReader::new(f).split(b'\n') {
            let Ok(line) = line else { break };
            let digits = line.iter().take_while(|c| c.is_ascii_digit()).count();
            if line.get(digits) != Some(&b' ') {
                report_error!(
                    "error in file {}: index not followed by space in line '{}'\n",
                    self.dirtab_file.display(),
                    lossy(&line)
                );
                // like the C server: keep what was loaded so far
                break;
            }
            let index: usize = std::str::from_utf8(&line[..digits])
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let path = fsutil::unescape(&line[digits + 1..]);
            elems.push(self.new_element(&path, index, default_watch));
            next_index = next_index.max(index + 1);
        }
        // the file is written sorted, but don't rely on it for the binary search
        elems.sort_by(|a, b| a.abspath.cmp(&b.abspath));
        elems.dedup_by(|a, b| a.abspath == b.abspath);
        t.elems = elems;
        t.next_index = next_index;
    }

    /// Writes `<root>/dirtab` if anything was added since the last flush.
    pub fn flush(&self) -> bool {
        // never hold the table lock while taking element locks
        let elems = {
            let mut t = self.table();
            if !t.dirty {
                return true;
            }
            t.dirty = false;
            t.elems.clone()
        };
        let mut buf = Vec::new();
        for el in &elems {
            if el.lock().state != State::Removed {
                buf.extend_from_slice(el.index.to_string().as_bytes());
                buf.push(b' ');
                buf.extend_from_slice(&fsutil::escape(&el.abspath));
                buf.push(b'\n');
            }
        }
        // write to a temp file and rename, so a crash never leaves a truncated dirtab
        let tmp = self.root.join("dirtab.tmp");
        let res = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
            .and_then(|mut f| f.write_all(&buf))
            .and_then(|_| fs::rename(&tmp, &self.dirtab_file));
        if let Err(e) = res {
            report_error!(
                "error opening {} for writing: {}\n",
                self.dirtab_file.display(),
                fsutil::strerror(e.raw_os_error().unwrap_or(0))
            );
            return false;
        }
        true
    }

    /// Finds or creates the element for `abspath`.
    pub fn get_element(&self, abspath: &[u8]) -> Arc<Element> {
        let mut t = self.table();
        match t
            .elems
            .binary_search_by(|e| e.abspath.as_slice().cmp(abspath))
        {
            Ok(i) => t.elems[i].clone(),
            Err(i) => {
                let index = t.next_index;
                t.next_index += 1;
                let el = self.new_element(abspath, index, WatchState::None);
                t.elems.insert(i, el.clone());
                t.dirty = true;
                el
            }
        }
    }

    /// Snapshot of all elements (in path order), so visitors run without the table lock.
    pub fn snapshot(&self) -> Vec<Arc<Element>> {
        self.table().elems.clone()
    }

    pub fn is_empty(&self) -> bool {
        self.table().elems.is_empty()
    }
}
