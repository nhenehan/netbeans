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

//! pty_open: allocates a pseudo terminal, reports it, and relays bytes between
//! its own stdin/stdout and the pty master until stdin goes away. Port of
//! `tools/pty_open.c` (Linux). `PtyAllocator` starts it, reads the header that
//! `PtyOpenUtility.readSatelliteOutput` parses, and hands the slave name to the
//! debugger as the debuggee's terminal.

use std::ffi::CStr;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

/// The C tool's buffer size (glibc BUFSIZ).
const BUFSIZ: usize = 8192;

/// An open pty: the master, and the slave kept open so the pty stays usable
/// (reading the master would fail with EIO once no slave is open).
pub struct Pty {
    pub master: File,
    pub slave: File,
    pub name: String,
}

/// posix_openpt + grantpt + unlockpt + ptsname, then opens the slave.
/// The error says which step failed, as the C tool's messages did.
pub fn open_pty() -> Result<Pty, (&'static str, io::Error)> {
    // SAFETY: plain libc calls on the fd we just got; ptsname's static buffer is
    // copied before any other call can overwrite it (single-threaded here)
    unsafe {
        let fd = libc::posix_openpt(libc::O_RDWR);
        if fd == -1 {
            return Err(("ptm_open() failed", io::Error::last_os_error()));
        }
        let master = File::from(OwnedFd::from_raw_fd(fd));
        if libc::grantpt(fd) == -1 || libc::unlockpt(fd) == -1 {
            return Err(("ptm_open() failed", io::Error::last_os_error()));
        }
        let p = libc::ptsname(fd);
        if p.is_null() {
            return Err(("ptsname() failed", io::Error::last_os_error()));
        }
        let name = CStr::from_ptr(p).to_string_lossy().into_owned();
        // the same flags as the C tool (O_RDWR, no O_NOCTTY)
        let slave = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&name)
            .map_err(|e| ("cannot open PTY slave", e))?;
        Ok(Pty {
            master,
            slave,
            name,
        })
    }
}

/// The header `PtyOpenUtility.readSatelliteOutput` expects.
pub fn header(pid: u32, tty: &str) -> String {
    format!("PID: {pid}\nTTY: {tty}\n\n")
}

/// Relays stdin -> master and master -> stdout until stdin reaches EOF or
/// either side hangs up. Pending input is read before a hangup ends the loop
/// (the C tool stopped at POLLHUP even with bytes still readable).
pub fn relay(input: &mut File, master: &mut File, output: &mut impl Write) -> Result<(), String> {
    let mut buf = [0u8; BUFSIZ];
    let mut fds = [
        libc::pollfd {
            fd: input.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: master.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    loop {
        // SAFETY: fds is a valid array of two pollfd
        if unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) } == -1 {
            let e = io::Error::last_os_error();
            // an interrupted poll is ignored, see CR 7086177
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(format!("poll() failed in main_loop: {e}"));
        }

        if fds[0].revents & libc::POLLIN != 0 {
            let n = read_retry(input, &mut buf)
                .map_err(|e| format!("read from stdin failed: {e}"))?;
            if n == 0 {
                return Ok(());
            }
            master
                .write_all(&buf[..n])
                .map_err(|e| format!("write to master failed: {e}"))?;
        }

        if fds[1].revents & libc::POLLIN != 0 {
            let n = read_retry(master, &mut buf)
                .map_err(|e| format!("read from master failed: {e}"))?;
            if n == 0 {
                return Ok(());
            }
            output
                .write_all(&buf[..n])
                .and_then(|_| output.flush())
                .map_err(|e| format!("write to stdout failed: {e}"))?;
        }

        let hup = libc::POLLHUP | libc::POLLERR | libc::POLLNVAL;
        if (fds[0].revents & hup != 0 && fds[0].revents & libc::POLLIN == 0)
            || (fds[1].revents & hup != 0 && fds[1].revents & libc::POLLIN == 0)
        {
            return Ok(());
        }
    }
}

/// read(2), retried when a signal interrupts it.
fn read_retry(f: &mut File, buf: &mut [u8]) -> io::Result<usize> {
    loop {
        match f.read(buf) {
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            r => return r,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_format() {
        assert_eq!(header(42, "/dev/pts/7"), "PID: 42\nTTY: /dev/pts/7\n\n");
    }

    #[test]
    fn opens_a_pty() {
        let pty = open_pty().map_err(|(m, e)| format!("{m}: {e}")).unwrap();
        assert!(pty.name.starts_with("/dev/pts/"), "{}", pty.name);
        // SAFETY: isatty on an fd we own
        assert_eq!(unsafe { libc::isatty(pty.slave.as_raw_fd()) }, 1);
    }
}
