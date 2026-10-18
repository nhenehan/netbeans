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

//! Runs pty (nbstart) the way NbNativeProcess does. Set PTY_C to a C pty
//! (e.g. Linux-x86_64/pty from the 24aefa9 zip) to also compare the two on
//! the cases where they must agree.

use std::collections::HashMap;
use std::ffi::CStr;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

const PTY: &str = env!("CARGO_BIN_EXE_pty");
const KILLALL: &str = env!("CARGO_BIN_EXE_killall");

fn tmpdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pty-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Runs to completion with stdin closed. Returns (exit code, header, rest of stdout, stderr).
fn run(exe: &str, args: &[&str]) -> (Option<i32>, HashMap<String, String>, String, String) {
    let out = Command::new(exe)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let (h, rest) = split_header(&String::from_utf8_lossy(&out.stdout));
    (
        out.status.code(),
        h,
        rest,
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Splits pty's stdout into the KEY=VALUE header (up to the empty line) and
/// the rest; tolerates the \r\n a pty adds.
fn split_header(s: &str) -> (HashMap<String, String>, String) {
    let mut h = HashMap::new();
    let mut lines = s.split_inclusive('\n');
    for line in lines.by_ref() {
        let l = line.trim();
        if l.is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once('=') {
            h.insert(k.to_string(), v.to_string());
        }
    }
    (h, lines.collect())
}

// ---- --no-pty: the most common mode (builds, tools, clangd) -------------

#[test]
fn no_pty_header_and_output() {
    let child = Command::new(PTY)
        .args(["--no-pty", "/bin/sh", "-c", "echo hi; echo err >&2"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pty_pid = child.id().to_string();
    let out = child.wait_with_output().unwrap();
    let (h, rest) = split_header(&String::from_utf8_lossy(&out.stdout));
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(h["TTY"], "null");
    // its own session (stdout is not a tty), magic is the pty's pid
    assert_eq!(h["PSID"], h["PID"]);
    assert_eq!(h["NBMAGIC"], pty_pid);
    assert!(!h.contains_key("REPORT"));
    assert_eq!(rest, "hi\n");
    assert_eq!(String::from_utf8_lossy(&out.stderr), "err\n");
}

#[test]
fn exit_codes() {
    assert_eq!(
        run(PTY, &["--no-pty", "/bin/sh", "-c", "exit 3"]).0,
        Some(3)
    );
    // killed by a signal: 1
    assert_eq!(
        run(PTY, &["--no-pty", "/bin/sh", "-c", "kill -TERM $$"]).0,
        Some(1)
    );
}

#[test]
fn exec_and_chdir_failures() {
    let (code, h, _, err) = run(PTY, &["--no-pty", "/no/such/program"]);
    assert_eq!(code, Some(127));
    assert!(h.contains_key("PID"), "header comes first");
    assert_eq!(
        err,
        "failed to start /no/such/program: No such file or directory\n"
    );

    let (code, _, _, err) = run(PTY, &["--dir", "/no/such/dir", "--no-pty", "/bin/true"]);
    assert_eq!(code, Some(127));
    assert_eq!(
        err,
        "failed to change directory: /no/such/dir: No such file or directory\n"
    );

    let (code, _, out, _) = run(PTY, &["--dir", "/", "--no-pty", "/bin/pwd"]);
    assert_eq!((code, out.as_str()), (Some(0), "/\n"));
}

#[test]
fn env_and_magic() {
    let (_, h, out, _) = run(
        PTY,
        &[
            "--no-pty",
            "--env",
            "FOO=bar",
            "--env",
            "NBMAGIC=spoofed",
            "--env",
            "FOO=baz",
            "/bin/sh",
            "-c",
            "echo $FOO $NBMAGIC",
        ],
    );
    // later --env wins, NBMAGIC is always ours
    assert_eq!(out, format!("baz {}\n", h["NBMAGIC"]));
}

#[test]
fn redirect_error() {
    let (_, _, out, err) = run(
        PTY,
        &[
            "--no-pty",
            "--redirect-error",
            "/bin/sh",
            "-c",
            "echo e >&2",
        ],
    );
    assert_eq!((out.as_str(), err.as_str()), ("e\n", ""));
}

#[test]
fn usage_and_bad_options() {
    let out = Command::new(PTY).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("usage: pty "));
    let out = Command::new(PTY).arg("--bogus").output().unwrap();
    assert_eq!(out.status.code(), Some(255));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "ERROR unrecognized option '--bogus'\n"
    );
    let out = Command::new(PTY).arg("--dir").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "missing dir after --dir\n\n"
    );
}

// ---- NBMAGIC: how Stop kills the whole tree -------------------------------

#[test]
fn magic_reaches_grandchildren() {
    let mut child = Command::new(PTY)
        .args(["--no-pty", "/bin/sh", "-c", "sleep 30 & sleep 30; wait"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut header = Vec::new();
    let mut b = [0u8; 1];
    let out = child.stdout.as_mut().unwrap();
    while !header.ends_with(b"\n\n") {
        out.read_exact(&mut b).unwrap();
        header.push(b[0]);
    }
    let (h, _) = split_header(&String::from_utf8_lossy(&header));
    let magic = format!("NBMAGIC={}", h["NBMAGIC"]);
    std::thread::sleep(Duration::from_millis(200));
    let listed = Command::new(KILLALL)
        .args(["-n", "-e", "NULL", &magic])
        .output()
        .unwrap();
    let pids: Vec<&str> = std::str::from_utf8(&listed.stdout)
        .unwrap()
        .lines()
        .collect();
    assert_eq!(pids.len(), 3, "sh and both sleeps: {pids:?}");
    assert!(
        !pids.contains(&child.id().to_string().as_str()),
        "pty itself has no magic"
    );
    Command::new(KILLALL)
        .args(["-e", "TERM", &magic])
        .status()
        .unwrap();
    assert_eq!(child.wait().unwrap().code(), Some(1));
}

// ---- --report: project Run's "RUN FINISHED" line --------------------------

#[test]
fn report_file() {
    let d = tmpdir("report");
    let status = d.join("status");
    let s = status.to_str().unwrap();
    let child = Command::new(PTY)
        .args(["--report", s, "--no-pty", "/bin/sh", "-c", "exit 4"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let pty_pid = child.id();
    let out = child.wait_with_output().unwrap();
    let (h, _) = split_header(&String::from_utf8_lossy(&out.stdout));
    assert_eq!(h["REPORT"], format!("{s}.{pty_pid}"));
    let r = std::fs::read_to_string(&h["REPORT"]).unwrap();
    let mut lines = r.lines();
    assert_eq!(lines.next(), Some("RC: 4"));
    assert_eq!(lines.next(), Some("SIG: -"));
    assert_eq!(lines.next(), Some("CORE: 0"));
    for (l, key) in lines.zip(["RTIME", "UTIME", "STIME", "MAXRSS"]) {
        let (k, v) = l.split_once(": ").unwrap();
        assert_eq!(k, key);
        v.parse::<i64>().unwrap();
    }

    let (code, h, _, _) = run(
        PTY,
        &["--report", s, "--no-pty", "/bin/sh", "-c", "kill -TERM $$"],
    );
    assert_eq!(code, Some(1));
    let r = std::fs::read_to_string(&h["REPORT"]).unwrap();
    assert!(r.starts_with("RC: 1\nSIG: Terminated\nCORE: 0\n"), "{r}");
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn report_times_are_milliseconds() {
    let d = tmpdir("rtime");
    let s = d.join("status");
    let (_, h, _, _) = run(
        PTY,
        &[
            "--report",
            s.to_str().unwrap(),
            "--no-pty",
            "/bin/sleep",
            "0.3",
        ],
    );
    let r = std::fs::read_to_string(&h["REPORT"]).unwrap();
    let rtime: i64 = r
        .lines()
        .find_map(|l| l.strip_prefix("RTIME: "))
        .unwrap()
        .parse()
        .unwrap();
    assert!((250..3000).contains(&rtime), "RTIME {rtime} ms");
    std::fs::remove_dir_all(d).unwrap();
}

// ---- --dumpenv / --readenv: remote host setup and every remote process ----

#[test]
fn dumpenv_then_readenv() {
    let d = tmpdir("env");
    let f = d.join("env");
    let fs = f.to_str().unwrap();
    let st = Command::new(PTY)
        .env_clear()
        .env("FROM_LOGIN", "1")
        .env("PATH", "/usr/bin:/bin")
        .args(["--dumpenv", fs])
        .status()
        .unwrap();
    assert_eq!(st.code(), Some(0));

    let out = Command::new(PTY)
        .env("NOT_FROM_LOGIN", "1")
        .args(["--readenv", fs, "--env", "EXTRA=2", "--no-pty", "env"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let (h, rest) = split_header(&String::from_utf8_lossy(&out.stdout));
    let mut env: Vec<&str> = rest.lines().collect();
    env.sort();
    let magic = format!("NBMAGIC={}", h["NBMAGIC"]);
    let mut want = vec![
        "EXTRA=2",
        "FROM_LOGIN=1",
        "PATH=/usr/bin:/bin",
        magic.as_str(),
    ];
    want.sort();
    assert_eq!(
        env, want,
        "only the recorded environment, plus --env and NBMAGIC"
    );
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn readenv_failure_is_an_error() {
    // the C tool started the program with an empty environment instead
    let (code, h, _, err) = run(PTY, &["--readenv", "/no/such/env", "--no-pty", "/bin/true"]);
    assert_eq!(code, Some(1));
    assert!(h.is_empty());
    assert!(
        err.starts_with("failed to read environment from /no/such/env"),
        "{err}"
    );
}

#[test]
fn dumpenv_reports_a_held_lock() {
    let d = tmpdir("lock");
    let f = d.join("env");
    std::fs::write(&f, b"").unwrap();
    // fcntl locks are per process: hold the write lock in a forked child
    let (mut ready_r, ready_w) = {
        let mut fds = [0; 2];
        // SAFETY: pipe into a valid array
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        // SAFETY: fresh fds we own
        unsafe { (File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1])) }
    };
    let path = std::ffi::CString::new(f.to_str().unwrap()).unwrap();
    // SAFETY: the child only makes async-signal-safe calls, then _exits
    let holder = unsafe { libc::fork() };
    if holder == 0 {
        unsafe {
            let fd = libc::open(path.as_ptr(), libc::O_RDWR);
            let mut fl: libc::flock = std::mem::zeroed();
            fl.l_type = libc::F_WRLCK as libc::c_short;
            libc::fcntl(fd, libc::F_SETLK, &fl);
            libc::write(ready_w.as_raw_fd(), b"x".as_ptr().cast(), 1);
            libc::sleep(10);
            libc::_exit(0);
        }
    }
    drop(ready_w);
    let mut b = [0u8; 1];
    ready_r.read_exact(&mut b).unwrap();
    let st = Command::new(PTY).args(["--dumpenv", f.to_str().unwrap()]).status().unwrap();
    // SAFETY: ending and reaping our own child
    unsafe {
        libc::kill(holder, libc::SIGKILL);
        libc::waitpid(holder, std::ptr::null_mut(), 0);
    }
    assert_eq!(st.code(), Some(1), "locked by another process");
    assert_eq!(std::fs::read(&f).unwrap(), b"", "not truncated or written while locked");
    std::fs::remove_dir_all(d).unwrap();
}

// ---- pty mode: run/debug in the IDE terminal ------------------------------

struct Session {
    child: Child,
    stdin: Option<ChildStdin>,
    out: Receiver<u8>,
    seen: Vec<u8>,
}

impl Session {
    fn start(exe: &str, args: &[&str]) -> Session {
        let mut child = Command::new(exe)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        let mut stdout = child.stdout.take().unwrap();
        let (tx, out) = channel();
        std::thread::spawn(move || {
            let mut b = [0u8; 4096];
            while let Ok(n) = stdout.read(&mut b) {
                if n == 0 {
                    break;
                }
                for &c in &b[..n] {
                    if tx.send(c).is_err() {
                        return;
                    }
                }
            }
        });
        Session {
            child,
            stdin,
            out,
            seen: Vec::new(),
        }
    }

    fn read_until(&mut self, end: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        let from = self.seen.len();
        while !self.seen[from..].ends_with(end.as_bytes()) {
            match self
                .out
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            {
                Ok(b) => self.seen.push(b),
                Err(_) => panic!(
                    "timed out waiting for {end:?}; got {:?}",
                    String::from_utf8_lossy(&self.seen[from..])
                ),
            }
        }
        String::from_utf8_lossy(&self.seen[from..]).into_owned()
    }

    fn header(&mut self) -> HashMap<String, String> {
        split_header(&self.read_until("\r\n\r\n")).0
    }

    fn send(&mut self, b: &[u8]) {
        let s = self.stdin.as_mut().unwrap();
        s.write_all(b).unwrap();
        s.flush().unwrap();
    }

    fn wait(&mut self, within: Duration) -> Option<i32> {
        let deadline = Instant::now() + within;
        loop {
            if let Some(st) = self.child.try_wait().unwrap() {
                return st.code();
            }
            if Instant::now() > deadline {
                let _ = self.child.kill();
                panic!("pty did not exit within {within:?}");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Everything still to come on stdout, until EOF.
    fn rest(&mut self) -> Vec<u8> {
        let mut v = Vec::new();
        while let Ok(b) = self.out.recv_timeout(Duration::from_secs(10)) {
            v.push(b);
        }
        v
    }
}

#[test]
fn pty_mode_session() {
    let mut s = Session::start(
        PTY,
        &[
            "--set-erase-key",
            "/bin/sh",
            "-c",
            "tty; stty -a | grep -o 'erase = [^;]*'; read x; echo got:$x",
        ],
    );
    let h = s.header();
    assert!(h["TTY"].starts_with("/dev/pts/"), "{h:?}");
    assert_eq!(h["PSID"], h["PID"], "session leader on its own terminal");
    let t = s.read_until("erase = ^H\r\n");
    assert!(t.starts_with(&format!("{}\r\n", h["TTY"])), "{t:?}");
    s.send(b"abc\n");
    s.read_until("abc\r\n"); // echo
    s.read_until("got:abc\r\n");
    assert_eq!(s.wait(Duration::from_secs(5)), Some(0));
}

#[test]
fn pty_mode_exit_code_and_ctty() {
    // the terminal is the controlling tty: /dev/tty opens
    let mut s = Session::start(PTY, &["/bin/sh", "-c", "echo x > /dev/tty && exit 5"]);
    s.header();
    s.read_until("x\r\n");
    assert_eq!(s.wait(Duration::from_secs(5)), Some(5));
}

#[test]
fn pty_mode_large_output_is_drained() {
    // the C tool stopped at the first POLLHUP and could lose the tail
    let mut s = Session::start(
        PTY,
        &["/bin/sh", "-c", "head -c 300000 /dev/zero | tr '\\0' a"],
    );
    s.header();
    let rest = s.rest();
    assert_eq!(rest.iter().filter(|&&b| b == b'a').count(), 300000);
    assert_eq!(s.wait(Duration::from_secs(5)), Some(0));
}

#[test]
fn pty_mode_hangup_ends_the_program_quickly() {
    let mut s = Session::start(PTY, &["/bin/sleep", "30"]);
    s.header();
    let t0 = Instant::now();
    drop(s.stdin.take()); // the IDE goes away
    assert_eq!(s.wait(Duration::from_secs(5)), Some(1));
    // one SIGTERM is enough for sleep; the C tool always waited 3 s
    assert!(
        t0.elapsed() < Duration::from_millis(1500),
        "{:?}",
        t0.elapsed()
    );
}

#[test]
fn pty_mode_stdin_eof_without_hangup_keeps_output() {
    // stdin at EOF but not hung up (/dev/null): output must still flow
    let out = Command::new(PTY)
        .args(["/bin/sh", "-c", "sleep 0.3; echo late"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("late\r\n"));
    assert_eq!(out.status.code(), Some(0));
}

// ---- -p: run on a given terminal (gdb's pty_open terminal) ----------------

fn open_pty() -> (File, String) {
    // SAFETY: standard pty allocation on fds we own
    unsafe {
        let m = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
        assert!(m >= 0);
        assert_eq!(libc::grantpt(m), 0);
        assert_eq!(libc::unlockpt(m), 0);
        let name = CStr::from_ptr(libc::ptsname(m))
            .to_string_lossy()
            .into_owned();
        (File::from_raw_fd(m), name)
    }
}

#[test]
fn given_pty() {
    let (mut master, name) = open_pty();
    // keep a slave open so the master doesn't EIO once the program is done
    let _keep = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags_noctty()
        .open(&name)
        .unwrap();
    let out = Command::new(PTY)
        .args(["-p", &name, "/bin/sh", "-c", "tty; exit 2"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty(), "everything goes to the terminal");
    let mut buf = vec![0u8; 4096];
    let n = master.read(&mut buf).unwrap();
    let t = String::from_utf8_lossy(&buf[..n]).into_owned();
    let (h, rest) = split_header(&t);
    assert_eq!(h["TTY"], name);
    assert!(
        rest.starts_with(&format!("{name}\r\n")) || rest.is_empty(),
        "{t:?}"
    );
}

trait NoCtty {
    fn custom_flags_noctty(&mut self) -> &mut Self;
}
impl NoCtty for OpenOptions {
    fn custom_flags_noctty(&mut self) -> &mut Self {
        use std::os::unix::fs::OpenOptionsExt;
        self.custom_flags(libc::O_NOCTTY)
    }
}

// ---- -w: wait for SIGCONT before exec ------------------------------------

#[test]
fn wait_for_sigcont() {
    let mut child = Command::new(PTY)
        .args(["-w", "--no-pty", "/bin/echo", "go"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut out = child.stdout.take().unwrap();
    let mut header = Vec::new();
    let mut b = [0u8; 1];
    while !header.ends_with(b"\n\n") {
        out.read_exact(&mut b).unwrap();
        header.push(b[0]);
    }
    let (h, _) = split_header(&String::from_utf8_lossy(&header));
    // send it at once: the C tool could lose a SIGCONT that came this early
    Command::new(KILLALL)
        .args(["-p", "CONT", &h["PID"]])
        .status()
        .unwrap();
    let mut rest = String::new();
    out.read_to_string(&mut rest).unwrap();
    assert_eq!(rest, "go\n");
    assert_eq!(child.wait().unwrap().code(), Some(0));
}

// ---- the C tool ----------------------------------------------------------

/// The cases where C and Rust must agree, with pids masked.
fn comparable(exe: &str) -> Vec<(Option<i32>, Vec<String>, String, String)> {
    let cases: &[&[&str]] = &[
        &["--no-pty", "/bin/sh", "-c", "echo hi; exit 3"],
        &["--no-pty", "/bin/sh", "-c", "kill -TERM $$"],
        &["--no-pty", "/no/such/program"],
        &["--dir", "/no/such/dir", "--no-pty", "/bin/true"],
        &[
            "--dir",
            "/",
            "--no-pty",
            "--env",
            "A=1",
            "--env",
            "A=2",
            "/bin/sh",
            "-c",
            "pwd; echo $A",
        ],
        &[
            "--no-pty",
            "--redirect-error",
            "/bin/sh",
            "-c",
            "echo e >&2",
        ],
        &["--bogus"],
        &["--dir"],
        &[],
    ];
    cases
        .iter()
        .map(|a| {
            let (code, h, rest, err) = run(exe, a);
            let mut keys: Vec<String> = h.keys().cloned().collect();
            keys.sort();
            let mask = |s: &str| {
                let mut s = s.to_string();
                for k in ["PID", "PSID", "NBMAGIC"] {
                    if let Some(v) = h.get(k) {
                        s = s.replace(v, "<pid>");
                    }
                }
                s
            };
            (code, keys, mask(&rest), mask(&err))
        })
        .collect()
}

#[test]
fn same_as_c() {
    let Some(c) = std::env::var("PTY_C").ok() else {
        eprintln!("PTY_C not set; skipping the comparison with the C pty");
        return;
    };
    let rs = comparable(PTY);
    let cc = comparable(&c);
    for (i, (r, c)) in rs.iter().zip(&cc).enumerate() {
        assert_eq!(r, c, "case {i}");
    }
}

/// A pty-mode session with stdin kept open, as the IDE runs it. (With stdin
/// at EOF the C tool stopped relaying and printed nothing at all, not even the
/// header; see pty_mode_stdin_eof_without_hangup_keeps_output.)
fn pty_session(exe: &str) -> (Vec<String>, String, Option<i32>) {
    let mut s = Session::start(exe, &["--set-erase-key", "/bin/sh", "-c",
        "read x; echo got:$x; stty -a | grep -o 'erase = [^;]*'; exit 6"]);
    let h = s.header();
    let mut keys: Vec<String> = h.keys().cloned().collect();
    keys.sort();
    s.send(b"abc\n");
    let t = s.read_until("erase = ^H\r\n");
    (keys, t, s.wait(Duration::from_secs(5)))
}

#[test]
fn same_as_c_in_pty_mode() {
    let Some(c) = std::env::var("PTY_C").ok() else {
        eprintln!("PTY_C not set; skipping the comparison with the C pty");
        return;
    };
    assert_eq!(pty_session(PTY), pty_session(&c));
}
