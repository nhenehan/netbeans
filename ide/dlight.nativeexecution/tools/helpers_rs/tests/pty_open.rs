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

//! Drives pty_open the way PtyAllocator and a debuggee do: read the header,
//! open the slave, and pass bytes both ways. Set PTY_OPEN_C to a C pty_open
//! to also run the same session against it and compare the transcripts.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

const PTY_OPEN: &str = env!("CARGO_BIN_EXE_pty_open");

struct Session {
    child: Child,
    stdin: Option<ChildStdin>,
    out: Receiver<u8>,
    pid: u32,
    tty: String,
}

impl Session {
    fn start(exe: &str) -> Session {
        let mut child = Command::new(exe)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        let mut stdout = child.stdout.take().unwrap();
        let (tx, out) = channel();
        std::thread::spawn(move || {
            let mut b = [0u8; 1];
            while let Ok(1) = stdout.read(&mut b) {
                if tx.send(b[0]).is_err() {
                    break;
                }
            }
        });
        let mut s = Session {
            child,
            stdin,
            out,
            pid: 0,
            tty: String::new(),
        };
        let header = s.read_until("\n\n");
        // the format PtyOpenUtility.readSatelliteOutput parses
        let mut lines = header.lines();
        s.pid = lines
            .next()
            .unwrap()
            .strip_prefix("PID: ")
            .unwrap()
            .parse()
            .unwrap();
        s.tty = lines
            .next()
            .unwrap()
            .strip_prefix("TTY: ")
            .unwrap()
            .to_string();
        assert_eq!(lines.next(), Some(""));
        s
    }

    /// Bytes from pty_open's stdout up to and including `end`.
    fn read_until(&self, end: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut got = Vec::new();
        while !got.ends_with(end.as_bytes()) {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.out.recv_timeout(left) {
                Ok(b) => got.push(b),
                Err(_) => panic!(
                    "timed out waiting for {end:?}, got {:?}",
                    String::from_utf8_lossy(&got)
                ),
            }
        }
        String::from_utf8(got).unwrap()
    }

    fn open_slave(&self) -> File {
        OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY)
            .open(&self.tty)
            .unwrap()
    }

    fn send(&mut self, bytes: &[u8]) {
        let stdin = self.stdin.as_mut().unwrap();
        stdin.write_all(bytes).unwrap();
        stdin.flush().unwrap();
    }

    /// Closes stdin and returns the exit code.
    fn finish(mut self) -> Option<i32> {
        drop(self.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(st) = self.child.try_wait().unwrap() {
                return st.code();
            }
            if Instant::now() > deadline {
                let _ = self.child.kill();
                panic!("pty_open did not exit after stdin was closed");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// A canonical session: debuggee output, then typed input (echoed by the line
/// discipline), then EOF on stdin. Returns everything pty_open wrote after
/// the header, and its exit code.
fn transcript(exe: &str) -> (String, Option<i32>) {
    let mut s = Session::start(exe);
    assert!(s.tty.starts_with("/dev/pts/"), "{}", s.tty);
    assert_eq!(s.pid, s.child.id());
    let mut slave = s.open_slave();

    slave.write_all(b"name?\n").unwrap();
    let mut t = s.read_until("name?\r\n"); // ONLCR on the slave side

    s.send(b"world\n");
    let mut line = [0u8; 64];
    let n = slave.read(&mut line).unwrap();
    assert_eq!(&line[..n], b"world\n");
    t += &s.read_until("world\r\n"); // echo

    slave.write_all(b"hello world\n").unwrap();
    t += &s.read_until("hello world\r\n");

    (t, s.finish())
}

#[test]
fn header_relay_and_eof() {
    let (t, code) = transcript(PTY_OPEN);
    assert_eq!(t, "name?\r\nworld\r\nhello world\r\n");
    assert_eq!(code, Some(0));
}

#[test]
fn survives_debuggee_closing_the_slave() {
    // the debuggee opens and closes the tty; pty_open keeps its own slave open,
    // so the pty stays usable for the next one
    let mut s = Session::start(PTY_OPEN);
    drop(s.open_slave());
    let mut slave = s.open_slave();
    slave.write_all(b"again\n").unwrap();
    s.read_until("again\r\n");
    s.send(b"x\n");
    let mut b = [0u8; 8];
    assert_eq!(slave.read(&mut b).unwrap(), 2);
    assert_eq!(s.finish(), Some(0));
}

#[test]
fn large_input_is_not_truncated() {
    let mut s = Session::start(PTY_OPEN);
    let mut slave = s.open_slave();
    // raw mode on the slave so the line discipline neither echoes nor limits lines
    // SAFETY: termios calls on an fd we own
    unsafe {
        use std::os::fd::AsRawFd;
        let mut t: libc::termios = std::mem::zeroed();
        libc::tcgetattr(slave.as_raw_fd(), &mut t);
        libc::cfmakeraw(&mut t);
        libc::tcsetattr(slave.as_raw_fd(), libc::TCSANOW, &t);
    }
    let data: Vec<u8> = (0..3000u32).map(|i| b'a' + (i % 26) as u8).collect();
    s.send(&data);
    let mut got = Vec::new();
    let mut b = [0u8; 4096];
    while got.len() < data.len() {
        let n = slave.read(&mut b).unwrap();
        got.extend_from_slice(&b[..n]);
    }
    assert_eq!(got, data);
    assert_eq!(s.finish(), Some(0));
}

#[test]
fn same_as_c() {
    let Some(c) = std::env::var("PTY_OPEN_C").ok() else {
        eprintln!("PTY_OPEN_C not set; skipping the comparison with the C pty_open");
        return;
    };
    assert_eq!(transcript(PTY_OPEN), transcript(&c));
}

#[test]
fn started_through_sh_like_ptyallocator() {
    // PtyAllocator runs `sh -s` and writes `exec "<pty_open>"` to it; whatever
    // follows on the same stream must reach pty_open, not the shell
    let mut child = Command::new("/bin/sh")
        .arg("-s")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    stdin.write_all(format!("exec \"{PTY_OPEN}\"\n").as_bytes()).unwrap();
    stdin.flush().unwrap();

    let mut header = Vec::new();
    let mut b = [0u8; 1];
    while !header.ends_with(b"\n\n") {
        assert_eq!(stdout.read(&mut b).unwrap(), 1);
        header.push(b[0]);
    }
    let header = String::from_utf8(header).unwrap();
    let pid: u32 = header.lines().next().unwrap()[5..].parse().unwrap();
    assert_eq!(pid, child.id(), "exec keeps the shell's pid");
    let tty = header.lines().nth(1).unwrap()[5..].to_string();

    let mut slave = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOCTTY)
        .open(&tty)
        .unwrap();
    stdin.write_all(b"typed\n").unwrap();
    stdin.flush().unwrap();
    let mut line = [0u8; 16];
    let n = slave.read(&mut line).unwrap();
    assert_eq!(&line[..n], b"typed\n");

    drop(stdin);
    assert_eq!(child.wait().unwrap().code(), Some(0));
}
