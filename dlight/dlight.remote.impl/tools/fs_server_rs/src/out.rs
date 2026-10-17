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

//! Output channels: protocol responses on stdout, diagnostics on stderr
//! (optionally redirected to a file) and the optional request log.
//!
//! Every protocol line is written with a single call under the stdout lock,
//! so lines produced by different worker threads never interleave mid-line.

use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::mem::ManuallyDrop;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::FromRawFd;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Mutex, OnceLock};

pub const TRACE_NONE: i32 = 0;
pub const TRACE_INFO: i32 = 1;
pub const TRACE_FINE: i32 = 2;
pub const TRACE_FINER: i32 = 3;
pub const TRACE_FINEST: i32 = 4;

static BROKEN_PIPE: AtomicBool = AtomicBool::new(false);
static TRACE_LEVEL: AtomicI32 = AtomicI32::new(TRACE_NONE);

/// fd 1 without std's line buffering; never closed.
struct Fd1(ManuallyDrop<File>);

impl Write for Fd1 {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (&*self.0).write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// stdout, fully buffered; flushed explicitly at the end of each response.
fn stdout() -> &'static Mutex<BufWriter<Fd1>> {
    static OUT: OnceLock<Mutex<BufWriter<Fd1>>> = OnceLock::new();
    // SAFETY: fd 1 is open for the life of the process; ManuallyDrop keeps us from closing it.
    OUT.get_or_init(|| {
        Mutex::new(BufWriter::with_capacity(
            64 * 1024,
            Fd1(ManuallyDrop::new(unsafe { File::from_raw_fd(1) })),
        ))
    })
}

/// stderr or the file given with -e.
static ERR_FILE: Mutex<Option<File>> = Mutex::new(None);
/// request log opened with -l.
static LOG_FILE: Mutex<Option<File>> = Mutex::new(None);

pub fn is_broken_pipe() -> bool {
    BROKEN_PIPE.load(Ordering::Relaxed)
}

pub fn set_broken_pipe() {
    BROKEN_PIPE.store(true, Ordering::Relaxed);
}

fn check_io(r: io::Result<()>) {
    if let Err(e) = r {
        if e.kind() == io::ErrorKind::BrokenPipe {
            set_broken_pipe();
        }
    }
}

/// Writes one protocol line (must include the trailing '\n').
pub fn out_line(line: &[u8]) {
    if is_broken_pipe() {
        return;
    }
    let mut w = stdout().lock().unwrap_or_else(|p| p.into_inner());
    check_io(w.write_all(line));
}

pub fn out_flush() {
    if is_broken_pipe() {
        return;
    }
    let mut w = stdout().lock().unwrap_or_else(|p| p.into_inner());
    check_io(w.flush());
}

pub fn set_trace(level: i32) {
    TRACE_LEVEL.store(level, Ordering::Relaxed);
}

pub fn trace_level() -> i32 {
    TRACE_LEVEL.load(Ordering::Relaxed)
}

pub fn is_traceable(level: i32) -> bool {
    trace_level() >= level
}

/// Redirects diagnostics to `file` (appending, mode 0600); `None` means stderr.
pub fn redirect_err(file: Option<&Path>) {
    let mut guard = ERR_FILE.lock().unwrap_or_else(|p| p.into_inner());
    *guard = None;
    if let Some(path) = file {
        match OpenOptions::new()
            .append(true)
            .create(true)
            .mode(0o600)
            .open(path)
        {
            Ok(f) => *guard = Some(f),
            Err(e) => {
                drop(guard);
                report_error(&format!(
                    "Error redirecting stderr to {}: {}",
                    path.display(),
                    crate::fsutil::strerror(e.raw_os_error().unwrap_or(0))
                ));
            }
        }
    }
}

fn err_write(bytes: &[u8]) {
    let mut guard = ERR_FILE.lock().unwrap_or_else(|p| p.into_inner());
    let r = match guard.as_mut() {
        Some(f) => f.write_all(bytes).and_then(|_| f.flush()),
        None => {
            let mut e = io::stderr().lock();
            e.write_all(bytes).and_then(|_| e.flush())
        }
    };
    check_io(r);
}

fn prefix() -> String {
    // SAFETY: getpid/pthread_self have no preconditions.
    let (pid, tid) = unsafe { (libc::getpid(), libc::pthread_self()) };
    format!("fs_server [{} {}]: ", pid, tid as i64)
}

/// Same as the C report_error(): always printed (unless the pipe is broken).
pub fn report_error(msg: &str) {
    if is_broken_pipe() {
        return;
    }
    let mut s = prefix();
    s.push_str(msg);
    err_write(s.as_bytes());
}

pub fn trace(level: i32, msg: &str) {
    if is_traceable(level) && !is_broken_pipe() {
        let mut s = prefix();
        s.push_str(msg);
        err_write(s.as_bytes());
    }
}

/// Plain text to the diagnostics stream, no prefix (usage, statistics, sleep notices).
pub fn err_print(msg: &str) {
    if !is_broken_pipe() {
        err_write(msg.as_bytes());
    }
}

pub fn log_open(path: &Path) {
    let f = OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .open(path)
        .ok();
    *LOG_FILE.lock().unwrap_or_else(|p| p.into_inner()) = f;
}

pub fn log_close() {
    *LOG_FILE.lock().unwrap_or_else(|p| p.into_inner()) = None;
}

pub fn log_print(bytes: &[u8]) {
    let mut guard = LOG_FILE.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(f) = guard.as_mut() {
        let _ = f.write_all(bytes).and_then(|_| f.flush());
    }
}

/// Writes to both the diagnostics stream and the request log (startup header).
pub fn log_and_err_print(msg: &str) {
    err_write(msg.as_bytes());
    log_print(msg.as_bytes());
}

#[macro_export]
macro_rules! report_error {
    ($($arg:tt)*) => { $crate::out::report_error(&format!($($arg)*)) };
}

#[macro_export]
macro_rules! trace {
    ($level:expr, $($arg:tt)*) => {
        if $crate::out::is_traceable($level) {
            $crate::out::trace($level, &format!($($arg)*))
        }
    };
}
