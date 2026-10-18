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

//! The parent's relay for a newly allocated pty (loop.c): stdin -> master,
//! master -> stdout, without ever blocking on the master. Differences from
//! the C tool, all fixes:
//! - EIO from the master (Linux, once the program has closed the terminal)
//!   ends the relay like EOF; the C tool exited 1 there, before reaping.
//! - Output still buffered when the program closes the terminal is drained,
//!   not cut at the first POLLHUP.
//! - Input from the IDE that arrives together with its hangup is passed on.
//! - EOF on a stdin that does not hang up (e.g. /dev/null) only stops reading
//!   stdin; the C tool also stopped relaying the program's output.
//! - Writes to the master wait on POLLOUT in the main poll instead of
//!   spinning in 1 ms polls.

use std::io;

/// Why the relay ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Ended {
    /// the program side closed the terminal (it exited): reap it
    ChildClosed,
    /// stdin hung up (the IDE went away): close the master, end the program
    Hangup,
}

const BUFSIZ: usize = 8192;

fn read(fd: i32, buf: &mut [u8]) -> io::Result<usize> {
    loop {
        // SAFETY: read into a valid buffer
        let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
        if n >= 0 {
            return Ok(n as usize);
        }
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

fn write(fd: i32, buf: &[u8]) -> io::Result<usize> {
    loop {
        // SAFETY: write from a valid buffer
        let n = unsafe { libc::write(fd, buf.as_ptr().cast(), buf.len()) };
        if n >= 0 {
            return Ok(n as usize);
        }
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

fn write_all(fd: i32, mut buf: &[u8]) -> io::Result<()> {
    while !buf.is_empty() {
        let n = write(fd, buf)?;
        buf = &buf[n..];
    }
    Ok(())
}

fn is_eio(e: &io::Error) -> bool {
    e.raw_os_error() == Some(libc::EIO)
}

/// Relays until the program closes the terminal or stdin hangs up.
/// `master` is switched to non-blocking.
pub fn relay(input: i32, master: i32, output: i32) -> io::Result<Ended> {
    // SAFETY: fcntl on an fd we own
    unsafe {
        let fl = libc::fcntl(master, libc::F_GETFL);
        libc::fcntl(master, libc::F_SETFL, fl | libc::O_NONBLOCK);
    }
    let mut buf = [0u8; BUFSIZ];
    let mut pending: Vec<u8> = Vec::new();
    let mut input_open = true;

    loop {
        let mut fds = [
            libc::pollfd {
                // while the program hasn't taken the last chunk, ask for no
                // events: poll still reports a hangup, but we read no more
                fd: if input_open { input } else { -1 },
                events: if pending.is_empty() { libc::POLLIN } else { 0 },
                revents: 0,
            },
            libc::pollfd {
                fd: master,
                events: libc::POLLIN | if pending.is_empty() { 0 } else { libc::POLLOUT },
                revents: 0,
            },
        ];
        // SAFETY: fds is a valid array of two pollfd
        if unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) } == -1 {
            let e = io::Error::last_os_error();
            // an interrupted poll is ignored, see CR 7086177
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e);
        }

        let m = fds[1].revents;
        if m & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0 {
            // drain what the program wrote; EIO/0 means it closed the terminal
            loop {
                match read(master, &mut buf) {
                    Ok(0) => return Ok(Ended::ChildClosed),
                    Ok(n) => {
                        if write_all(output, &buf[..n]).is_err() {
                            // nobody reads our output any more: same as a hangup
                            return Ok(Ended::Hangup);
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        if m & libc::POLLHUP != 0 {
                            return Ok(Ended::ChildClosed);
                        }
                        break;
                    }
                    Err(e) if is_eio(&e) => return Ok(Ended::ChildClosed),
                    Err(e) => return Err(e),
                }
            }
        }
        if m & libc::POLLOUT != 0 && !pending.is_empty() {
            match write(master, &pending) {
                Ok(n) => {
                    pending.drain(..n);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(e) if is_eio(&e) => return Ok(Ended::ChildClosed),
                Err(e) => return Err(e),
            }
        }

        let i = fds[0].revents;
        let hup = i & (libc::POLLHUP | libc::POLLERR) != 0;
        if hup && !pending.is_empty() {
            // the IDE went away while the program wasn't reading
            return Ok(Ended::Hangup);
        }
        if i & libc::POLLIN != 0 || hup {
            match read(input, &mut buf)? {
                0 if hup => return Ok(Ended::Hangup),
                0 => input_open = false,
                n => {
                    pending.extend_from_slice(&buf[..n]);
                    match write(master, &pending) {
                        Ok(w) => {
                            pending.drain(..w);
                        }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                        Err(e) if is_eio(&e) => return Ok(Ended::ChildClosed),
                        Err(e) => return Err(e),
                    }
                }
            }
        } else if i & libc::POLLNVAL != 0 {
            input_open = false;
        }
    }
}
