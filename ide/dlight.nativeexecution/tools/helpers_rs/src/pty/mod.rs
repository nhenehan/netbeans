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

//! pty ("nbstart"): starts a program for NbNativeProcess, on a new pseudo
//! terminal, on a given one (`-p`), or on its own stdio (`--no-pty`), reports
//! PID/TTY/session, relays the terminal, and passes the exit code on. Port of
//! tools/pty/src (Linux); see `plan/n2-pty-survey.md` for the protocol and the
//! C bugs fixed here.

pub mod envfile;
pub mod options;
pub mod report;
pub mod relay;

use options::Options;
use std::ffi::{CStr, CString, OsString};
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::sync::atomic::{AtomicI64, Ordering};

/// When the child announced (SIGCONT) that it is about to exec, in ms of
/// CLOCK_MONOTONIC; 0 = not yet.
static START_MS: AtomicI64 = AtomicI64::new(0);

// the casts widen time_t/c_long on 32-bit targets (i686); no-ops on 64-bit
#[allow(clippy::unnecessary_cast)]
pub fn monotonic_ms() -> i64 {
    // SAFETY: clock_gettime with a valid timespec; async-signal-safe
    unsafe {
        let mut ts: libc::timespec = std::mem::zeroed();
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts);
        ts.tv_sec as i64 * 1000 + ts.tv_nsec as i64 / 1_000_000
    }
}

extern "C" fn on_sigcont(_: libc::c_int) {
    START_MS.store(monotonic_ms(), Ordering::Relaxed);
}

pub fn start_ms() -> i64 {
    START_MS.load(Ordering::Relaxed)
}

/// Installs the SIGCONT handler that takes the start time. Set before the fork,
/// so the child's SIGCONT cannot arrive before it.
pub fn install_sigcont_handler() {
    // SAFETY: sigaction with a handler that only does async-signal-safe work
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = on_sigcont as *const () as libc::sighandler_t;
        sa.sa_flags = libc::SA_RESTART;
        libc::sigemptyset(&mut sa.sa_mask);
        libc::sigaction(libc::SIGCONT, &sa, std::ptr::null_mut());
    }
}

pub fn set_signal(sig: libc::c_int, handler: libc::sighandler_t) {
    // SAFETY: SIG_IGN/SIG_DFL dispositions
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = handler;
        libc::sigemptyset(&mut sa.sa_mask);
        libc::sigaction(sig, &sa, std::ptr::null_mut());
    }
}

/// The environment the program gets: `base` (ours, or the `--readenv` file),
/// then each `--env NAME=VALUE` replacing any entry with that name (putenv),
/// then NBMAGIC last. An entry without `=` removes the name, as glibc putenv
/// does.
pub fn build_env(base: Vec<Vec<u8>>, envvars: &[OsString], magic: &[u8]) -> Vec<Vec<u8>> {
    fn name(e: &[u8]) -> &[u8] {
        e.split(|&b| b == b'=').next().unwrap_or(e)
    }
    let mut env = base;
    for v in envvars.iter().map(|v| v.as_bytes()).chain(std::iter::once(magic)) {
        let n = name(v);
        env.retain(|e| name(e) != n);
        if v.contains(&b'=') {
            env.push(v.to_vec());
        }
    }
    env
}

pub fn current_env() -> Vec<Vec<u8>> {
    std::env::vars_os()
        .map(|(k, v)| {
            let mut e = k.into_vec();
            e.push(b'=');
            e.extend_from_slice(v.as_bytes());
            e
        })
        .collect()
}

/// The header NbNativeProcess.readProcessInfo reads, up to the empty line.
pub fn header(pid: i32, tty: Option<&[u8]>, psid: i32, report: Option<&[u8]>, ppid: i32) -> Vec<u8> {
    let mut h = format!("PID={pid}\nTTY=").into_bytes();
    h.extend_from_slice(tty.unwrap_or(b"null"));
    h.extend_from_slice(format!("\nPSID={psid}\n").as_bytes());
    if let Some(r) = report {
        h.extend_from_slice(b"REPORT=");
        h.extend_from_slice(r);
        h.extend_from_slice(format!(".{ppid}\n").as_bytes());
    }
    h.extend_from_slice(format!("NBMAGIC={ppid}\n\n").as_bytes());
    h
}

/// Everything the child needs, prepared before the fork.
pub struct ChildPlan {
    pub argv: Vec<CString>,
    pub envp: Vec<CString>,
    pub wdir: Option<CString>,
    pub report: Option<Vec<u8>>,
    pub pty: Option<CString>,
    pub noecho: bool,
    pub set_erase_key: bool,
    pub wait_signal: bool,
}

impl ChildPlan {
    pub fn new(o: &Options, program: &[OsString], base_env: Vec<Vec<u8>>) -> io::Result<ChildPlan> {
        let c = |b: &[u8]| CString::new(b).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e));
        // SAFETY: getpid has no preconditions; the child's parent is us
        let magic = format!("NBMAGIC={}", unsafe { libc::getpid() });
        Ok(ChildPlan {
            argv: program.iter().map(|a| c(a.as_bytes())).collect::<io::Result<_>>()?,
            envp: build_env(base_env, &o.envvars, magic.as_bytes())
                .iter()
                .map(|e| c(e))
                .collect::<io::Result<_>>()?,
            wdir: o.wdir.as_ref().map(|d| c(d.as_bytes())).transpose()?,
            report: o.reportfile.as_ref().map(|r| r.as_bytes().to_vec()),
            pty: o.pty.as_ref().map(|p| c(p.as_bytes())).transpose()?,
            noecho: o.noecho,
            set_erase_key: o.set_erase_key,
            wait_signal: o.wait_signal,
        })
    }
}

/// Error output in the child, as the C err_sys: `<msg>: <strerror>` (pty's
/// messages, unlike killall's, have no program name in front). Written
/// straight to fd 2 (no Rust stdio state is trusted after fork).
pub fn child_fail(code: i32, msg: &str, err: Option<io::Error>) -> ! {
    let mut s = msg.to_string();
    if let Some(e) = err {
        s.push_str(&format!(": {}", strerror(&e)));
    }
    s.push('\n');
    write_fd(2, s.as_bytes());
    // SAFETY: _exit skips atexit handlers and stdio flushing, right after fork
    unsafe { libc::_exit(code) }
}

/// The C wording of an errno (std's Display adds " (os error N)").
pub fn strerror(e: &io::Error) -> String {
    match e.raw_os_error() {
        // SAFETY: strerror returns a valid C string
        Some(n) => unsafe { CStr::from_ptr(libc::strerror(n)) }.to_string_lossy().into_owned(),
        None => e.to_string(),
    }
}

pub fn write_fd(fd: i32, mut data: &[u8]) {
    while !data.is_empty() {
        // SAFETY: write from a valid buffer
        let n = unsafe { libc::write(fd, data.as_ptr().cast(), data.len()) };
        if n < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return;
        }
        data = &data[n as usize..];
    }
}

/// The child after fork, once its stdio is final: header, terminal mode,
/// environment, optional wait for SIGCONT, notify the parent, chdir, exec.
/// SIGCONT is blocked from the start, so a SIGCONT sent right after the header
/// (`-w`) is kept pending instead of lost before sigsuspend.
pub fn run_child(plan: &ChildPlan) -> ! {
    // SAFETY: the process is single-threaded (we forked from a single thread),
    // and every pointer below comes from the prepared plan
    unsafe {
        let mut cont: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut cont);
        libc::sigaddset(&mut cont, libc::SIGCONT);
        libc::sigprocmask(libc::SIG_BLOCK, &cont, std::ptr::null_mut());

        let tty = if libc::isatty(1) == 1 {
            let p = libc::ttyname(1);
            (!p.is_null()).then(|| CStr::from_ptr(p).to_bytes().to_vec())
        } else {
            plan.pty.as_ref().map(|p| p.to_bytes().to_vec())
        };
        let ppid = libc::getppid();
        let h = header(libc::getpid(), tty.as_deref(), libc::getsid(0), plan.report.as_deref(), ppid);
        write_fd(1, &h);

        modify_term(0, plan.noecho, plan.set_erase_key);

        if plan.wait_signal {
            // wait for SIGCONT; everything else stays blocked (queued) except SIGINT
            let mut mask: libc::sigset_t = std::mem::zeroed();
            libc::sigfillset(&mut mask);
            libc::sigdelset(&mut mask, libc::SIGCONT);
            libc::sigdelset(&mut mask, libc::SIGINT);
            libc::sigsuspend(&mask);
        }
        set_signal(libc::SIGCONT, libc::SIG_DFL);
        libc::sigprocmask(libc::SIG_UNBLOCK, &cont, std::ptr::null_mut());

        // tell the parent that the program starts now (report RTIME)
        libc::kill(ppid, libc::SIGCONT);

        if let Some(d) = &plan.wdir {
            if libc::chdir(d.as_ptr()) == -1 {
                let e = io::Error::last_os_error();
                let code = if e.raw_os_error() == Some(libc::ENOENT) { 127 } else { 1 };
                let msg = format!("failed to change directory: {}", d.to_string_lossy());
                child_fail(code, &msg, Some(e));
            }
        }

        let mut argv: Vec<*const libc::c_char> = plan.argv.iter().map(|a| a.as_ptr()).collect();
        argv.push(std::ptr::null());
        let mut envp: Vec<*const libc::c_char> = plan.envp.iter().map(|e| e.as_ptr()).collect();
        envp.push(std::ptr::null());
        // execvp looks the program up in the PATH of `environ`, so switch to
        // the program's environment first
        environ = envp.as_ptr() as *mut *mut libc::c_char;
        libc::execvp(argv[0], argv.as_ptr());
        let e = io::Error::last_os_error();
        let code = if e.raw_os_error() == Some(libc::ENOENT) { 127 } else { 1 };
        let msg = format!("failed to start {}", plan.argv[0].to_string_lossy());
        child_fail(code, &msg, Some(e));
    }
}

extern "C" {
    static mut environ: *mut *mut libc::c_char;
}

/// `-e`: echo off and no NL -> CR/NL on output; `--set-erase-key`: VERASE = ^H.
fn modify_term(fd: i32, echooff: bool, fixerase: bool) {
    if !echooff && !fixerase {
        return;
    }
    // SAFETY: termios calls on fd with a valid struct
    unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(fd, &mut t) < 0 {
            child_fail(1, "tcgetattr error", Some(io::Error::last_os_error()));
        }
        if echooff {
            t.c_lflag &= !(libc::ECHO | libc::ECHOE | libc::ECHOK | libc::ECHONL);
            t.c_oflag &= !libc::ONLCR;
        }
        if fixerase {
            t.c_cc[libc::VERASE] = 0x08;
        }
        if libc::tcsetattr(fd, libc::TCSANOW, &t) < 0 {
            child_fail(1, "tcsetattr error", Some(io::Error::last_os_error()));
        }
    }
}

/// In the child: make `slave` the controlling terminal and fds 0-2 (dup_fd).
pub fn attach_terminal(slave: i32) {
    // SAFETY: plain syscalls on fds we own
    unsafe {
        // the program must not inherit an ignored SIGINT
        set_signal(libc::SIGINT, libc::SIG_DFL);
        if libc::ioctl(slave, libc::TIOCSCTTY, 0) == -1 {
            let e = io::Error::last_os_error();
            let msg = format!("ERROR ioctl(TIOCSCTTY) failed on \"pty {slave}\" -- {}\n", strerror(&e));
            write_fd(1, msg.as_bytes());
            libc::_exit(255);
        }
        for (fd, what) in [(0, "stdin"), (1, "stdout"), (2, "stderr")] {
            if libc::dup2(slave, fd) != fd {
                child_fail(1, &format!("dup2 error to {what}"), Some(io::Error::last_os_error()));
            }
        }
        if slave > 2 {
            libc::close(slave);
        }
    }
}

/// Waits for `pid`, retrying on EINTR. None if it can't be waited for (the C
/// tool looped forever on ECHILD).
pub fn wait_child(pid: i32) -> Option<(i32, libc::rusage)> {
    loop {
        let mut status = 0;
        // SAFETY: wait4 with valid out-pointers
        let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
        let r = unsafe { libc::wait4(pid, &mut status, 0, &mut ru) };
        if r == pid {
            return Some((status, ru));
        }
        if r == -1 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
            continue;
        }
        return None;
    }
}

/// After the IDE side went away: SIGTERM up to 3 times, a second apart, then
/// SIGKILL. Stops as soon as the child has exited; the C tool probed with
/// kill(pid, 0), which also succeeds on a zombie, so it always took 3 s and
/// ended with SIGKILL.
pub fn terminate(pid: i32) -> Option<(i32, libc::rusage)> {
    let reaped = |flags| {
        let mut status = 0;
        // SAFETY: wait4 with valid out-pointers
        let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
        (unsafe { libc::wait4(pid, &mut status, flags, &mut ru) } == pid).then_some((status, ru))
    };
    for _ in 0..3 {
        if let Some(r) = reaped(libc::WNOHANG) {
            return Some(r);
        }
        // SAFETY: kill on our own child
        unsafe { libc::kill(pid, libc::SIGTERM) };
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            if let Some(r) = reaped(libc::WNOHANG) {
                return Some(r);
            }
        }
    }
    // SAFETY: kill on our own child
    unsafe { libc::kill(pid, libc::SIGKILL) };
    wait_child(pid)
}

#[allow(clippy::unnecessary_cast)] // see monotonic_ms
pub fn maxrss_kb(ru: &libc::rusage) -> i64 {
    ru.ru_maxrss as i64
}

#[allow(clippy::unnecessary_cast)] // see monotonic_ms
pub fn rusage_ms(tv: libc::timeval) -> i64 {
    tv.tv_sec as i64 * 1000 + tv.tv_usec as i64 / 1000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_order_and_override() {
        let base = vec![b"A=1".to_vec(), b"PATH=/bin".to_vec(), b"NBMAGIC=1".to_vec()];
        let env = build_env(
            base,
            &[OsString::from("PATH=/x"), OsString::from("B=2"), OsString::from("A")],
            b"NBMAGIC=42",
        );
        assert_eq!(env, vec![b"PATH=/x".to_vec(), b"B=2".to_vec(), b"NBMAGIC=42".to_vec()]);
    }

    #[test]
    fn header_lines() {
        assert_eq!(
            header(10, Some(b"/dev/pts/2"), 10, Some(b"/t/status"), 9),
            b"PID=10\nTTY=/dev/pts/2\nPSID=10\nREPORT=/t/status.9\nNBMAGIC=9\n\n".to_vec()
        );
        assert_eq!(header(5, None, 4, None, 3), b"PID=5\nTTY=null\nPSID=4\nNBMAGIC=3\n\n".to_vec());
    }
}
