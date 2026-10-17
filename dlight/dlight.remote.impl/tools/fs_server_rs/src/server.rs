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

//! Request processing: the reader loop, the worker pool, the refresh manager,
//! the persistence lock and orderly shutdown.

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::fs::{DirBuilderExt, FileExt, OpenOptionsExt};
use std::os::unix::io::AsRawFd;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::cli::{Options, MAX_THREAD_COUNT};
use crate::dirtab::{self, Element, RefreshState, State, WatchState};
use crate::exitcodes;
use crate::fsutil::{self, as_path, lossy};
use crate::out::{self, out_flush, out_line, TRACE_FINE, TRACE_FINER, TRACE_FINEST, TRACE_INFO};
use crate::protocol::{self as p, Entry, Request};
use crate::settings::{self, Settings};
use crate::{report_error, trace};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

const PERSISTENCE_VERSION_LABEL: &[u8] = b"VERSION=";
const PERSISTENCE_VERSION: i32 = 2;
const WORKER_STACK: usize = 16 * 1024 * 1024; // recursive ls descends one frame per level

// ---------------------------------------------------------------------------------------
// global state
// ---------------------------------------------------------------------------------------

static OPTS: OnceLock<Options> = OnceLock::new();
static PROCEED: AtomicBool = AtomicBool::new(true);
static BUSY: AtomicUsize = AtomicUsize::new(0);
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

fn opts() -> &'static Options {
    OPTS.get().expect("options not set")
}

fn need_to_proceed() -> bool {
    !out::is_broken_pipe() && PROCEED.load(Ordering::SeqCst)
}

struct Queue {
    inner: Mutex<QueueInner>,
    cond: Condvar,
}

struct QueueInner {
    items: VecDeque<Request>,
    shut_down: bool,
    max_size: usize,
}

static QUEUE: Queue = Queue {
    inner: Mutex::new(QueueInner {
        items: VecDeque::new(),
        shut_down: false,
        max_size: 0,
    }),
    cond: Condvar::new(),
};

impl Queue {
    fn lock(&self) -> std::sync::MutexGuard<'_, QueueInner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn add(&self, r: Request) {
        let mut q = self.lock();
        q.items.push_back(r);
        q.max_size = q.max_size.max(q.items.len());
        self.cond.notify_all();
    }

    /// Blocks until a request is available; None once shut down *and* drained.
    fn poll(&self) -> Option<Request> {
        let mut q = self.lock();
        loop {
            if let Some(r) = q.items.pop_front() {
                return Some(r);
            }
            if q.shut_down {
                return None;
            }
            q = self.cond.wait(q).unwrap_or_else(|p| p.into_inner());
        }
    }

    fn shutdown(&self) {
        self.lock().shut_down = true;
        self.cond.notify_all();
    }

    fn max_size(&self) -> usize {
        self.lock().max_size
    }
}

static WORKERS: Mutex<Vec<JoinHandle<()>>> = Mutex::new(Vec::new());
static REFRESH_THREAD: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);
/// wakes the refresh manager early on shutdown
static REFRESH_WAKE: (Mutex<()>, Condvar) = (Mutex::new(()), Condvar::new());
static DELETE_ON_EXIT: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
static LOCK_FILE: Mutex<Option<File>> = Mutex::new(None);

// ---------------------------------------------------------------------------------------
// responses
// ---------------------------------------------------------------------------------------

fn response_end(id: i32, path: &[u8]) {
    out_line(&p::path_line(p::RSP_END, id, path));
    out_flush();
}

fn response_error(id: i32, path: &[u8], err: i32, msg: &str) {
    out_line(&p::error_line(id, path, err, msg));
    out_flush();
}

fn entry_line(id: i32, entry_text: &[u8]) -> Vec<u8> {
    let mut line = format!("{} {} ", p::RSP_ENTRY as char, id).into_bytes();
    line.extend_from_slice(entry_text);
    line
}

fn open_cache_for_write(el: &Element, path: &[u8]) -> Option<File> {
    let f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&el.cache_path);
    match f {
        Ok(mut f) => {
            let mut head = Vec::new();
            head.extend_from_slice(PERSISTENCE_VERSION_LABEL);
            head.extend_from_slice(PERSISTENCE_VERSION.to_string().as_bytes());
            head.push(b'\n');
            head.extend_from_slice(&fsutil::escape(path));
            head.push(b'\n');
            if f.write_all(&head).is_err() {
                return None;
            }
            Some(f)
        }
        Err(e) => {
            report_error!(
                "error opening cache file for {}: {}\n",
                lossy(path),
                fsutil::strerror(e.raw_os_error().unwrap_or(0))
            );
            None
        }
    }
}

fn response_ls(id: i32, path: &[u8], recursive: bool, inner: bool, settings: &Settings) {
    if !need_to_proceed() {
        return;
    }
    let kind = if recursive {
        p::RSP_RECURSIVE_LS
    } else {
        p::RSP_LS
    };
    out_line(&p::path_line(kind, id, path));

    {
        let el = if opts().persistence {
            Some(dirtab::get().get_element(path))
        } else {
            None
        };
        let mut guard = el.as_ref().map(|el| el.lock());
        let mut cache = None;
        if let (Some(el), Some(g)) = (el.as_ref(), guard.as_mut()) {
            g.watch = WatchState::Poll;
            cache = open_cache_for_write(el, path);
        }

        let mut on_error = |dir_itself: bool, epath: &[u8], err: i32, msg: &str| {
            if dir_itself {
                response_error(id, epath, err, msg);
            } else {
                fsutil::default_error_handler(dir_itself, epath, err, msg);
            }
        };
        fsutil::visit_dir_entries(
            path,
            Some(settings),
            &mut on_error,
            &mut |name, meta, link, abspath| {
                // on NFS d_name may contain '/' or even be absolute; the C server skipped these too
                if name.contains(&b'/') {
                    report_error!("skipping entry {}\n", lossy(name));
                    return need_to_proceed();
                }
                let text = Entry::from_meta(name, meta, link, abspath, settings).format();
                out_line(&entry_line(id, &text));
                if let Some(f) = cache.as_mut() {
                    let _ = f.write_all(&text);
                }
                need_to_proceed()
            },
        );

        response_end(id, path);
        drop(cache);
        if let Some(g) = guard.as_mut() {
            g.state = State::LsSent;
        }
    }

    if recursive {
        fsutil::visit_dir_entries(
            path,
            Some(settings),
            &mut fsutil::default_error_handler,
            &mut |_name, meta, _link, abspath| {
                if meta.is_dir() {
                    response_ls(id, abspath, true, true, settings);
                }
                need_to_proceed()
            },
        );
        if !inner {
            response_end(id, path);
        }
    }
}

fn response_info(id: i32) {
    out_line(format!("{} {} {}\n", p::RSP_SERVER_INFO as char, id, VERSION).as_bytes());
    out_flush();
}

fn response_stat(id: i32, path: &[u8], settings: &Settings) {
    match fsutil::stat(path, Some(settings)) {
        Ok(meta) => {
            // unlike lstat, access() is consulted even for links (stat follows them anyway)
            let (r, w, x) = fsutil::get_access(path, &meta, settings);
            let e = Entry {
                name: fsutil::basename(path).to_vec(),
                file_type: meta.file_type_char(),
                size: meta.size,
                mtime: meta.mtime_ms,
                can_read: r,
                can_write: w,
                can_exec: x,
                dev: meta.dev,
                ino: meta.ino,
                link: Vec::new(),
            };
            out_line(&entry_line(id, &e.format()));
        }
        Err(err) => {
            let mut line = format!(
                "{} {} {} {}: ",
                p::RSP_ERROR as char,
                id,
                err,
                fsutil::strerror(err)
            )
            .into_bytes();
            line.extend_from_slice(path);
            line.push(b'\n');
            out_line(&line);
        }
    }
    out_flush();
}

fn response_lstat(id: i32, path: &[u8], settings: &Settings) {
    match p::lstat_entry(path, fsutil::basename(path), settings) {
        Ok(text) => out_line(&entry_line(id, &text)),
        Err((code, msg)) => {
            out_line(format!("{} {} {} {}\n", p::RSP_ERROR as char, id, code, msg).as_bytes())
        }
    }
    out_flush();
}

fn last_errno() -> i32 {
    io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

fn errno_of(e: &io::Error) -> i32 {
    e.raw_os_error().unwrap_or(0)
}

fn response_delete(id: i32, path: &[u8], settings: &Settings) {
    let Some(last_slash) = path.iter().rposition(|&c| c == b'/') else {
        response_error(id, path, 0, "wrong path");
        return;
    };
    if last_slash == 0 {
        response_error(id, path, 0, "won't remove '/'");
        return;
    }
    let canonical_parent = match fs::canonicalize(as_path(&path[..last_slash])) {
        Ok(pb) => {
            use std::os::unix::ffi::OsStrExt;
            pb.as_os_str().as_bytes().to_vec()
        }
        Err(e) => {
            response_error(
                id,
                path,
                errno_of(&e),
                "can't resolve parent canonical path",
            );
            return;
        }
    };
    match fsutil::lstat(path, Some(settings)) {
        Ok(meta) if meta.is_dir() => {
            if !fsutil::clean_dir(path) {
                response_error(id, path, last_errno(), "can't remote directory content");
                return;
            }
            if let Err(e) = fs::remove_dir(as_path(path)) {
                // reported, but (like the C server) the parent is still listed below
                response_error(id, path, errno_of(&e), "can't remove directory");
            }
        }
        Ok(_) => {
            if let Err(e) = fs::remove_file(as_path(path)) {
                response_error(id, path, errno_of(&e), "can't remove file");
                return;
            }
        }
        Err(err) => {
            response_error(id, path, err, "error getting stat");
            return;
        }
    }
    response_ls(id, &canonical_parent, false, false, settings);
}

fn response_delete_on_disconnect(id: i32, path: &[u8]) {
    if id != 0 {
        response_error(
            id,
            path,
            0,
            "FS_REQ_DELETE_ON_DISCONNECT request should have zero ID!",
        );
    }
    DELETE_ON_EXIT
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push(path.to_vec());
}

fn copy_plain_file(path: &[u8], path2: &[u8], id: i32) -> bool {
    let mut src = match File::open(as_path(path)) {
        Ok(f) => f,
        Err(e) => {
            response_error(id, path, errno_of(&e), &fsutil::strerror(errno_of(&e)));
            return false;
        }
    };
    let mut dst = match File::create(as_path(path2)) {
        Ok(f) => f,
        Err(e) => {
            response_error(id, path2, errno_of(&e), &fsutil::strerror(errno_of(&e)));
            return false;
        }
    };
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let n = match src.read(&mut buf) {
            Ok(0) => return true,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => {
                response_error(id, path, errno_of(&e), &fsutil::strerror(errno_of(&e)));
                return false;
            }
        };
        if let Err(e) = dst.write_all(&buf[..n]) {
            let err = errno_of(&e);
            if err != 0 {
                response_error(id, path2, err, &fsutil::strerror(err));
            } else {
                response_error(id, path2, 0, "error writing");
            }
            return false;
        }
    }
}

fn copy_symlink(path: &[u8], path2: &[u8], id: i32) -> bool {
    match fsutil::readlink(path) {
        Err(err) => {
            response_error(id, path, err, &fsutil::strerror(err));
            false
        }
        Ok(target) => match std::os::unix::fs::symlink(as_path(&target), as_path(path2)) {
            Ok(()) => true,
            Err(e) => {
                response_error(id, path2, errno_of(&e), &fsutil::strerror(errno_of(&e)));
                false
            }
        },
    }
}

fn copy_any(src: &[u8], meta: &fsutil::Meta, dst: &[u8], id: i32) -> bool {
    if meta.is_dir() {
        copy_dir(src, dst, id)
    } else if meta.is_reg() {
        copy_plain_file(src, dst, id)
    } else if meta.is_link() {
        copy_symlink(src, dst, id)
    } else {
        response_error(id, src, 0, "don't know how to copy a special file");
        false
    }
}

/// Like the C copy_dir(): stops at the first failing child but still reports success
/// unless the target directory itself could not be created.
fn copy_dir(path: &[u8], path2: &[u8], id: i32) -> bool {
    if let Err(e) = fs::DirBuilder::new().mode(0o700).create(as_path(path2)) {
        response_error(id, path2, errno_of(&e), &fsutil::strerror(errno_of(&e)));
        return false;
    }
    fsutil::visit_dir_entries(
        path,
        None,
        &mut fsutil::default_error_handler,
        &mut |name, meta, _link, abspath| {
            let mut dst = path2.to_vec();
            dst.push(b'/');
            dst.extend_from_slice(name);
            copy_any(abspath, meta, &dst, id)
        },
    );
    true
}

fn response_copy(req: &Request, settings: &Settings) {
    let path2 = req.path2.as_deref().unwrap_or_default();
    if fsutil::lstat(path2, Some(settings)).is_ok() {
        response_error(req.id, path2, 0, "file already exists");
        response_end(req.id, path2);
        return;
    }
    let meta = match fsutil::lstat(&req.path, Some(settings)) {
        Ok(m) => m,
        Err(err) => {
            response_error(req.id, &req.path, err, &fsutil::strerror(err));
            response_end(req.id, path2);
            return;
        }
    };
    let success = if meta.is_dir() || meta.is_reg() || meta.is_link() {
        copy_any(&req.path, &meta, path2, req.id)
    } else {
        response_error(req.id, path2, 0, "don't know how to copy a special file");
        false
    };
    if success {
        match path2.iter().rposition(|&c| c == b'/') {
            // response_ls() writes the end marker
            Some(i) => response_ls(req.id, &path2[..i], false, true, settings),
            None => {
                response_error(req.id, path2, 0, "path does not contain '/'");
                response_end(req.id, path2);
            }
        }
    } else {
        response_end(req.id, path2);
    }
}

fn response_move(req: &Request, settings: &Settings) {
    let path2 = req.path2.as_deref().unwrap_or_default();
    match fsutil::lstat(&req.path, Some(settings)) {
        Err(err) => {
            response_error(req.id, &req.path, err, &fsutil::strerror(err));
            return;
        }
        Ok(m) if m.is_dir() => {
            response_error(req.id, path2, 0, "can not move directory");
            return;
        }
        Ok(_) => {}
    }
    // copy + unlink (not rename): the C server does the same, so moves across file systems work
    if copy_plain_file(&req.path, path2, req.id) {
        if let Err(e) = fs::remove_file(as_path(&req.path)) {
            response_error(req.id, &req.path, errno_of(&e), "can't remove file");
            return;
        }
        response_lstat(req.id, path2, settings);
    }
}

fn response_add_or_remove_watch(path: &[u8], add: bool) {
    let el = dirtab::get().get_element(path);
    let mut g = el.lock();
    g.watch = if add {
        WatchState::Poll
    } else {
        WatchState::None
    };
    g.state = State::Initial;
}

fn response_help() {
    out_line(b"Help on request kinds\n");
    for (k, name) in p::REQUEST_KINDS {
        out_line(format!("{} - {}\n", *k as char, name).as_bytes());
    }
    out_flush();
}

// ---------------------------------------------------------------------------------------
// refresh manager
// ---------------------------------------------------------------------------------------

/// Reads `<cache>/<index>`; returns (entries, persistence version) or (None, version)
/// where version is -1 if the file could not be read at all.
fn read_entries_from_cache(el: &Element, path: &[u8]) -> (Option<Vec<Entry>>, i32) {
    let Ok(f) = File::open(&el.cache_path) else {
        return (None, -1);
    };
    let cache_name = el.cache_path.display().to_string();
    let mut lines = BufReader::new(f).split(b'\n');
    let Some(Ok(mut first)) = lines.next() else {
        report_error!(
            "error reading cache from {} for {}: preliminary EOF\n",
            cache_name,
            lossy(path)
        );
        return (None, -1);
    };
    let mut version = 1;
    if let Some(rest) = first.strip_prefix(PERSISTENCE_VERSION_LABEL) {
        match std::str::from_utf8(rest)
            .ok()
            .and_then(|s| s.trim().parse::<i32>().ok())
        {
            Some(v) => version = v,
            None => {
                report_error!(
                    "error reading cache from {} for {}: wrong version format: {}\n",
                    cache_name,
                    lossy(path),
                    lossy(&first)
                );
                return (None, version);
            }
        }
        if version != PERSISTENCE_VERSION {
            return (None, version);
        }
        match lines.next() {
            Some(Ok(l)) => first = l,
            _ => {
                report_error!(
                    "error reading cache from {} for {}: preliminary EOF\n",
                    cache_name,
                    lossy(path)
                );
                return (None, version);
            }
        }
    }
    if !fsutil::unescape(&first).starts_with(path) {
        report_error!(
            "error: first line in cache {} for {} is not '{}', but is '{}'\n",
            cache_name,
            lossy(path),
            lossy(path),
            lossy(&first)
        );
        return (None, version);
    }
    let mut entries = Vec::new();
    for line in lines {
        let Ok(mut line) = line else {
            report_error!(
                "error reading cache from {} for {}\n",
                cache_name,
                lossy(path)
            );
            return (None, version);
        };
        if line.is_empty() {
            continue;
        }
        trace!(TRACE_FINEST, "\tread entry: {}\n", lossy(&line));
        line.push(b'\n');
        match Entry::parse(&line) {
            Some(e) => entries.push(e),
            None => {
                report_error!(
                    "error reading entry from cache ({}): {}",
                    cache_name,
                    lossy(&line)
                );
                return (None, version);
            }
        }
    }
    (Some(entries), version)
}

fn read_entries_from_dir(path: &[u8], settings: &Settings) -> Vec<Entry> {
    let mut entries = Vec::new();
    fsutil::visit_dir_entries(
        path,
        Some(settings),
        &mut fsutil::default_error_handler,
        &mut |name, meta, link, abspath| {
            entries.push(Entry::from_meta(name, meta, link, abspath, settings));
            need_to_proceed()
        },
    );
    entries
}

fn entries_differ(new: &Entry, old: &Entry, version: i32, path: &[u8]) -> bool {
    let p = lossy(path);
    let n = lossy(&new.name);
    if new.name != old.name {
        trace!(
            TRACE_FINE,
            "refresh manager: names differ in directory {}: {} vs {}\n",
            p,
            n,
            lossy(&old.name)
        );
        return true;
    }
    if new.file_type != old.file_type {
        trace!(
            TRACE_FINE,
            "refresh manager: file types differ for {}/{}\n",
            p,
            n
        );
        return true;
    }
    if new.file_type == fsutil::FILETYPE_LNK && new.link != old.link {
        trace!(
            TRACE_FINE,
            "refresh manager: links differ for {}/{}\n",
            p,
            n
        );
        return true;
    }
    if new.file_type == fsutil::FILETYPE_REG {
        if new.size != old.size {
            trace!(
                TRACE_FINE,
                "refresh manager: sizes differ for {}/{}: {} vs {}\n",
                p,
                n,
                new.size,
                old.size
            );
            return true;
        }
        if new.mtime != old.mtime {
            trace!(
                TRACE_FINE,
                "refresh manager: times differ for {}/{}: {} vs {}\n",
                p,
                n,
                new.mtime,
                old.mtime
            );
            return true;
        }
    }
    if version > 1
        && (new.can_read != old.can_read
            || new.can_write != old.can_write
            || new.can_exec != old.can_exec
            || new.dev != old.dev
            || new.ino != old.ino)
    {
        trace!(
            TRACE_FINE,
            "refresh manager: access, device or inode differ for {}/{}\n",
            p,
            n
        );
        return true;
    }
    false
}

fn refresh_visit(el: &Element, req: Option<&Request>, settings: &Settings) -> bool {
    let path = el.abspath.as_slice();
    if fsutil::is_prohibited(path) {
        trace!(TRACE_FINER, "refresh manager: skipping {}\n", lossy(path));
        return need_to_proceed();
    }
    if let Some(r) = req {
        if !fsutil::is_subdir(path, &r.path) {
            trace!(TRACE_FINER, "refresh manager: skipping {}\n", lossy(path));
            return need_to_proceed();
        }
    }
    let mut g = el.lock();
    if req.is_none() && g.watch != WatchState::Poll {
        trace!(
            TRACE_FINER,
            "refresh manager: not polling {}\n",
            lossy(path)
        );
        return need_to_proceed();
    }
    if g.state == State::Removed {
        trace!(
            TRACE_FINER,
            "refresh manager: already marked as removed {}\n",
            lossy(path)
        );
        return need_to_proceed();
    }
    if !fsutil::dir_exists(path) {
        g.state = State::Removed;
        trace!(
            TRACE_FINER,
            "refresh manager: does not exist, marking as removed {}\n",
            lossy(path)
        );
        return need_to_proceed();
    }
    if req.is_none() && g.state == State::RefreshSent {
        trace!(
            TRACE_FINER,
            "refresh notification already sent for {}\n",
            lossy(path)
        );
        return need_to_proceed();
    }
    trace!(TRACE_FINER, "refresh manager: visiting {}\n", lossy(path));

    let (old, version) = read_entries_from_cache(el, path);
    let differs = match old {
        Some(mut old) => {
            let mut new = read_entries_from_dir(path, settings);
            old.sort_by(|a, b| a.name.cmp(&b.name));
            new.sort_by(|a, b| a.name.cmp(&b.name));
            new.len() != old.len()
                || new
                    .iter()
                    .zip(&old)
                    .any(|(n, o)| entries_differ(n, o, version, path))
        }
        None => {
            if version == -1 || version == PERSISTENCE_VERSION {
                // unreadable or corrupt cache; an old cache version is silently replaced
                report_error!("error refreshing {}: error reading cache\n", lossy(path));
            } else {
                trace!(
                    TRACE_FINEST,
                    "invalid cache (incorrect version: {}) for {}\n",
                    version,
                    lossy(path)
                );
            }
            true
        }
    };
    if differs {
        trace!(
            TRACE_INFO,
            "refresh manager: sending notification for {}\n",
            lossy(path)
        );
        out_line(&p::path_line(p::RSP_CHANGE, req.map_or(0, |r| r.id), path));
        out_flush();
        g.state = State::RefreshSent;
    }
    need_to_proceed()
}

fn refresh_cycle_impl(req: Option<&Request>, settings: &Settings) {
    let start = std::time::Instant::now();
    // a zero id means nobody waits for the result: no header and no end marker
    let reply = req.filter(|r| r.id != 0);
    if let Some(r) = reply {
        out_line(&p::path_line(p::RSP_REFRESH, r.id, &r.path));
    }
    for el in dirtab::get().snapshot() {
        if !refresh_visit(&el, req, settings) {
            break;
        }
    }
    if let Some(r) = reply {
        response_end(r.id, &r.path);
    }
    trace!(
        TRACE_FINE,
        "refresh cycle took {} ms\n",
        start.elapsed().as_millis()
    );
}

/// Coalesces concurrent refreshes of the same root: a refresh requested while one is
/// running sets a pending flag and the running thread repeats once more.
fn refresh_cycle(req: Option<&Request>, settings: &Settings) {
    dirtab::get().flush();
    let path: &[u8] = req.map_or(b"/", |r| r.path.as_slice());
    let el = dirtab::get().get_element(path);
    {
        let mut g = el.lock();
        match g.refresh {
            RefreshState::Refreshing => {
                trace!(
                    TRACE_FINEST,
                    "refresh[1] {} already refreshing: setting pending flag\n",
                    lossy(path)
                );
                g.refresh = RefreshState::PendingRefresh;
                return;
            }
            RefreshState::PendingRefresh => {
                trace!(
                    TRACE_FINEST,
                    "refresh[1] {} already pending: nothing to do\n",
                    lossy(path)
                );
                return;
            }
            RefreshState::None => g.refresh = RefreshState::Refreshing,
        }
    }
    loop {
        trace!(TRACE_FINEST, "refreshing {}...\n", lossy(path));
        refresh_cycle_impl(req, settings);
        let mut g = el.lock();
        if g.refresh == RefreshState::PendingRefresh {
            trace!(
                TRACE_FINEST,
                "refresh[2] {} pending request => once more\n",
                lossy(path)
            );
            g.refresh = RefreshState::Refreshing;
            continue;
        }
        g.refresh = RefreshState::None;
        return;
    }
}

/// Sleeps up to `secs`, returning early when shutdown starts.
fn refresh_sleep(secs: u64) {
    let (m, c) = &REFRESH_WAKE;
    let g = m.lock().unwrap_or_else(|p| p.into_inner());
    if need_to_proceed() {
        let _ = c.wait_timeout(g, Duration::from_secs(secs));
    }
}

fn refresh_loop() {
    let interval = opts().refresh_sleep;
    trace!(
        TRACE_INFO,
        "Refresh manager started; sleep interval is {}\n",
        interval
    );
    while need_to_proceed() && dirtab::get().is_empty() {
        refresh_sleep(if interval > 0 { interval } else { 2 });
    }
    let mut pass = 0;
    while need_to_proceed() {
        pass += 1;
        trace!(TRACE_FINE, "refresh manager, pass {}\n", pass);
        refresh_cycle(None, &settings::current());
        if interval > 0 {
            refresh_sleep(interval);
        }
    }
    trace!(TRACE_INFO, "Refresh manager stopped\n");
}

// ---------------------------------------------------------------------------------------
// dispatch
// ---------------------------------------------------------------------------------------

fn process_request(req: &Request) {
    let settings = settings::current();
    match req.kind {
        p::REQ_DELETE => response_delete(req.id, &req.path, &settings),
        p::REQ_DELETE_ON_DISCONNECT => response_delete_on_disconnect(req.id, &req.path),
        p::REQ_SERVER_INFO => response_info(req.id),
        p::REQ_LS => response_ls(req.id, &req.path, false, false, &settings),
        p::REQ_RECURSIVE_LS => response_ls(req.id, &req.path, true, false, &settings),
        p::REQ_STAT => response_stat(req.id, &req.path, &settings),
        p::REQ_LSTAT => response_lstat(req.id, &req.path, &settings),
        p::REQ_ADD_WATCH => response_add_or_remove_watch(&req.path, true),
        p::REQ_REMOVE_WATCH => response_add_or_remove_watch(&req.path, false),
        p::REQ_REFRESH => refresh_cycle(Some(req), &settings),
        p::REQ_COPY => response_copy(req, &settings),
        p::REQ_MOVE => response_move(req, &settings),
        p::REQ_HELP => response_help(),
        p::REQ_OPTION => settings::process_options(&req.path),
        k => report_error!("unexpected mode: '{}'\n", k as char),
    }
}

fn worker_loop(no: usize) {
    trace!(TRACE_FINE, "Thread #{} started\n", no);
    while !out::is_broken_pipe() {
        match QUEUE.poll() {
            Some(req) => {
                trace!(
                    TRACE_FINE,
                    "thread[{}] request #{} kind={} path={}\n",
                    no,
                    req.id,
                    req.kind as char,
                    lossy(&req.path)
                );
                BUSY.fetch_add(1, Ordering::SeqCst);
                process_request(&req);
                BUSY.fetch_sub(1, Ordering::SeqCst);
            }
            None => break,
        }
    }
    trace!(TRACE_FINE, "Thread #{} done\n", no);
}

fn start_worker(no: usize) {
    trace!(TRACE_FINE, "Starting thread #{}...\n", no);
    match thread::Builder::new()
        .name(format!("fs_server rp#{no}"))
        .stack_size(WORKER_STACK)
        .spawn(move || worker_loop(no))
    {
        Ok(h) => WORKERS.lock().unwrap_or_else(|p| p.into_inner()).push(h),
        Err(e) => report_error!("error starting thread #{}: {}\n", no, e),
    }
}

fn handle_sleep(req: &Request) {
    let digits: String = req
        .path
        .iter()
        .take_while(|c| c.is_ascii_digit())
        .map(|&c| c as char)
        .collect();
    let secs: u64 = digits.parse().unwrap_or(0);
    if secs > 0 {
        out::err_print(&format!("fs_server: sleeping {secs} seconds\n"));
        thread::sleep(Duration::from_secs(secs));
        out::err_print("fs_server: awoke\n");
    }
}

fn main_loop() {
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut raw = Vec::with_capacity(4096);
    let mut thread_count = opts().threads;
    while !out::is_broken_pipe() {
        raw.clear();
        match reader.read_until(b'\n', &mut raw) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
        trace!(TRACE_FINE, "request: {}", lossy(&raw));
        out::log_print(&raw);
        let Some(req) = p::decode_request(&raw) else {
            report_error!("incorrect request: {}", lossy(&raw));
            continue;
        };
        trace!(
            TRACE_FINEST,
            "decoded request #{} kind={} path={}\n",
            req.id,
            req.kind as char,
            lossy(&req.path)
        );
        match req.kind {
            p::REQ_QUIT => break,
            p::REQ_SLEEP => handle_sleep(&req),
            _ if thread_count > 1 => {
                if BUSY.load(Ordering::SeqCst) >= thread_count && thread_count < MAX_THREAD_COUNT {
                    start_worker(thread_count);
                    thread_count += 1;
                }
                QUEUE.add(req);
            }
            _ => process_request(&req),
        }
    }
}

// ---------------------------------------------------------------------------------------
// persistence lock
// ---------------------------------------------------------------------------------------

fn read_pid(f: &File) -> u32 {
    let mut buf = [0u8; 40];
    match f.read_at(&mut buf, 0) {
        Ok(n) if n > 4 && buf.starts_with(b"PID=") => {
            let s: String = buf[4..n]
                .iter()
                .take_while(|c| c.is_ascii_digit())
                .map(|&c| c as char)
                .collect();
            s.parse().unwrap_or(0)
        }
        _ => 0,
    }
}

fn write_pid(f: &File) {
    let _ = f.set_len(0);
    let _ = f.write_at(format!("PID={}\n", std::process::id()).as_bytes(), 0);
}

fn try_lock(f: &File) -> Result<(), i32> {
    // lockf(F_TLOCK) on the whole file, compatible with a C fs_server holding the same lock
    // SAFETY: valid fd; lockf has no memory-safety preconditions.
    let rc = unsafe {
        libc::lseek(f.as_raw_fd(), 0, libc::SEEK_SET);
        libc::lockf(f.as_raw_fd(), libc::F_TLOCK, 0)
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(last_errno())
    }
}

fn lock_persistence() {
    if !opts().persistence {
        return;
    }
    let root = dirtab::get().root().to_path_buf();
    let lock_path = root.join("lock");
    let f = match OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&lock_path)
    {
        Ok(f) => f,
        Err(e) => {
            report_error!(
                "error opening lock file {}: {}\n",
                lock_path.display(),
                fsutil::strerror(errno_of(&e))
            );
            std::process::exit(exitcodes::FAILURE_OPENING_LOCK_FILE);
        }
    };
    if let Err(mut lock_errno) = try_lock(&f) {
        let pid = read_pid(&f);
        if pid != 0 {
            let o = opts();
            if o.kill_wait_ms > 0 && (o.kill_pid == 0 || o.kill_pid == pid) {
                for signal in [libc::SIGTERM, libc::SIGKILL] {
                    trace!(
                        TRACE_INFO,
                        "Killing pid={} via sending signal {}...\n",
                        pid,
                        signal
                    );
                    // SAFETY: plain kill(2).
                    unsafe { libc::kill(pid as libc::pid_t, signal) };
                    for _ in 0..(o.kill_wait_ms / 10).max(1) {
                        match try_lock(&f) {
                            Ok(()) => {
                                write_pid(&f);
                                *LOCK_FILE.lock().unwrap_or_else(|p| p.into_inner()) = Some(f);
                                return;
                            }
                            Err(e) => {
                                lock_errno = e;
                                trace!(TRACE_FINE, "waiting for pid={} to terminate...\n", pid);
                                thread::sleep(Duration::from_millis(10));
                            }
                        }
                    }
                }
            }
            report_error!(
                "lock file already locked by PID={} {}: {}\n",
                pid,
                lock_path.display(),
                fsutil::strerror(lock_errno)
            );
        } else {
            report_error!(
                "error locking lock file {}: {}\n",
                lock_path.display(),
                fsutil::strerror(lock_errno)
            );
        }
        std::process::exit(exitcodes::FAILURE_LOCKING_LOCK_FILE);
    }
    write_pid(&f);
    *LOCK_FILE.lock().unwrap_or_else(|p| p.into_inner()) = Some(f);
}

// ---------------------------------------------------------------------------------------
// startup / shutdown
// ---------------------------------------------------------------------------------------

fn install_signal_handlers() {
    use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT};
    match signal_hook::iterator::Signals::new([SIGHUP, SIGINT, SIGQUIT]) {
        Ok(mut signals) => {
            let _ = thread::Builder::new()
                .name("fs_server signals".into())
                .spawn(move || {
                    if let Some(sig) = signals.forever().next() {
                        trace!(TRACE_INFO, "exiting by signal {}\n", sig);
                        shutdown_and_exit();
                    }
                });
        }
        Err(e) => {
            report_error!("error setting signal handler: {}\n", e);
            std::process::exit(exitcodes::FAILURE_SETTING_SIGNAL_HANDLER);
        }
    }
}

fn startup(opts_in: Options, root: std::path::PathBuf) {
    let threads = opts_in.threads;
    let refresh = opts_in.refresh;
    let watch = if opts_in.refresh_explicit {
        WatchState::None
    } else {
        WatchState::Poll
    };
    let clear = opts_in.clear_persistence;
    let persistence = opts_in.persistence;
    let default_error_log = opts_in.error_log == Some(None);
    OPTS.set(opts_in).expect("startup called twice");

    dirtab::init(root, clear, watch);
    let root = dirtab::get().root();
    if default_error_log {
        // opened only now: the directory exists and a -c cleanup can no longer unlink it
        out::redirect_err(Some(&root.join("stderr.txt")));
    }
    if let Err(e) = std::env::set_current_dir(root) {
        report_error!(
            "cannot change current directory to {}: {}\n",
            root.display(),
            fsutil::strerror(errno_of(&e))
        );
        std::process::exit(exitcodes::FAILED_CHDIR);
    }
    if persistence {
        trace!(TRACE_INFO, "Cache location: {}\n", root.display());
    } else {
        trace!(TRACE_INFO, "peristence is OFF\n");
    }
    lock_persistence();
    if out::is_traceable(TRACE_FINER) && !dirtab::get().is_empty() {
        trace!(TRACE_INFO, "loaded dirtab\n");
        for el in dirtab::get().snapshot() {
            trace!(TRACE_INFO, "{} {}\n", el.index, lossy(&el.abspath));
        }
    }
    if threads > 1 {
        for no in 0..threads {
            start_worker(no);
        }
        trace!(TRACE_INFO, "Started {} response threads\n", threads);
    } else {
        trace!(TRACE_INFO, "Starting in single-thread mode\n");
    }
    if refresh {
        match thread::Builder::new()
            .name("fs_server refresh".into())
            .stack_size(WORKER_STACK)
            .spawn(refresh_loop)
        {
            Ok(h) => *REFRESH_THREAD.lock().unwrap_or_else(|p| p.into_inner()) = Some(h),
            Err(e) => report_error!("error starting refresh thread: {}\n", e),
        }
    }
    install_signal_handlers();
}

fn delete_on_exit() {
    trace!(
        TRACE_INFO,
        "Processing files that should be deleted on exit\n"
    );
    let paths = std::mem::take(&mut *DELETE_ON_EXIT.lock().unwrap_or_else(|p| p.into_inner()));
    let (mut ok, mut failed) = (0, 0);
    for path in paths {
        trace!(TRACE_FINEST, "  removing {}...\n", lossy(&path));
        match fs::remove_file(as_path(&path)) {
            Ok(()) => ok += 1,
            Err(e) => {
                failed += 1;
                report_error!(
                    "error deleting file {}: {}\n",
                    lossy(&path),
                    fsutil::strerror(errno_of(&e))
                );
            }
        }
    }
    trace!(
        TRACE_INFO,
        "Successfully removed {} files; error removing {} files\n",
        ok,
        failed
    );
}

/// Stops workers (they drain the queue), runs delete-on-disconnect, stores the dirtab,
/// releases the lock and exits 0. A watchdog terminates the process if this takes > 2 s.
pub fn shutdown_and_exit() -> ! {
    if SHUTTING_DOWN.swap(true, Ordering::SeqCst) {
        // another thread is already shutting down; it will exit the process
        loop {
            thread::park();
        }
    }
    PROCEED.store(false, Ordering::SeqCst);
    QUEUE.shutdown();
    {
        let (m, c) = &REFRESH_WAKE;
        let _g = m.lock().unwrap_or_else(|p| p.into_inner());
        c.notify_all();
    }
    trace!(
        TRACE_INFO,
        "Max. requests queue size: {}\n",
        QUEUE.max_size()
    );
    if opts().statistics {
        out::err_print(&format!("Max. requests queue size: {}\n", QUEUE.max_size()));
    }
    let _ = thread::Builder::new()
        .name("fs_server watchdog".into())
        .spawn(|| {
            thread::sleep(Duration::from_secs(2));
            // SAFETY: plain kill(2) on ourselves; same as the C killer thread.
            unsafe { libc::kill(libc::getpid(), libc::SIGTERM) };
        });
    trace!(TRACE_INFO, "Shutting down. Joining threads...\n");
    let workers = std::mem::take(&mut *WORKERS.lock().unwrap_or_else(|p| p.into_inner()));
    for h in workers {
        let _ = h.join();
    }
    delete_on_exit();
    if let Some(h) = REFRESH_THREAD
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take()
    {
        let _ = h.join();
    }
    if !dirtab::get().flush() {
        report_error!("error storing dirtab\n");
    }
    out_flush();
    out::log_close();
    // closing the fd releases the lockf() lock
    LOCK_FILE.lock().unwrap_or_else(|p| p.into_inner()).take();
    trace!(TRACE_INFO, "Shut down.\n");
    std::process::exit(0);
}

fn log_header() {
    if !opts().log {
        return;
    }
    out::log_open(std::path::Path::new("log"));
    // SAFETY: time/localtime_r write only into `tm`.
    let when = unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            String::from("<error getting time>")
        } else {
            format!(
                "{}/{:02}/{:02} at {:02}:{:02}:{:02}",
                tm.tm_year + 1900,
                tm.tm_mon + 1,
                tm.tm_mday,
                tm.tm_hour,
                tm.tm_min,
                tm.tm_sec
            )
        }
    };
    let args: Vec<String> = std::env::args().collect();
    out::log_and_err_print(&format!(
        "\n--------------------------------------\nfs_server version {VERSION} [rust] started on {when}\n{} \n",
        args.join(" ")
    ));
}

/// Entry point after command line parsing.
pub fn run(opts_in: Options) -> ! {
    if let Some(level) = opts_in.trace_level {
        out::set_trace(level);
    }
    let root = dirtab::resolve_root(opts_in.persistence_dir.as_deref());
    // -e without a file name: <root>/stderr.txt, opened in startup() once root exists
    if let Some(Some(file)) = &opts_in.error_log {
        out::redirect_err(Some(std::path::Path::new(file)));
    }
    trace!(TRACE_INFO, "Version {} (rust)\n", VERSION);
    startup(opts_in, root);
    log_header();
    main_loop();
    shutdown_and_exit();
}
