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

use nativeexecution_helpers::pty::envfile::{self, DumpError};
use nativeexecution_helpers::pty::options::{readopts, usage, OptError};
use nativeexecution_helpers::pty::relay::{relay, Ended};
use nativeexecution_helpers::pty::report::{exit_code, format, Outcome};
use nativeexecution_helpers::pty::{
    attach_terminal, child_fail, current_env, install_sigcont_handler, monotonic_ms, run_child,
    maxrss_kb, rusage_ms, set_signal, start_ms, strerror, terminate, wait_child, ChildPlan,
};
use std::ffi::{CStr, CString, OsString};
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process;

/// err_quit: the message on stderr (pty's have no program name in front), exit 1.
fn err_quit(msg: &str) -> ! {
    let _ = io::stdout().flush();
    eprintln!("{msg}");
    process::exit(1);
}

/// err_sys: `<msg>: <strerror>`, exit 1.
fn err_sys(msg: &str) -> ! {
    let e = io::Error::last_os_error();
    err_quit(&format!("{msg}: {}", strerror(&e)));
}

/// A new pty: master fd and slave name, before the fork.
fn open_master() -> (i32, CString) {
    // SAFETY: plain libc calls; ptsname's buffer is copied at once
    unsafe {
        let fd = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC);
        if fd == -1 || libc::grantpt(fd) == -1 || libc::unlockpt(fd) == -1 {
            err_sys("ERROR: ptm_open() failed");
        }
        let p = libc::ptsname(fd);
        if p.is_null() {
            err_sys("ptsname failed");
        }
        (fd, CStr::from_ptr(p).to_owned())
    }
}

fn main() {
    let args: Vec<OsString> = std::env::args_os().collect();
    let progname = args
        .first()
        .and_then(|a| Path::new(a).file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "pty".into());

    let (opts, nopt) = match readopts(&args) {
        Ok(r) => r,
        Err(OptError::Unrecognized(opt)) => {
            println!("ERROR unrecognized option '{opt}'");
            process::exit(-1);
        }
        Err(OptError::Quit(msg)) => err_quit(msg),
    };

    if opts.dumpenv {
        let path = Path::new(opts.envfile.as_ref().unwrap());
        // stdout is parsed by host setup (the env dump that follows), so
        // errors only go to stderr
        match envfile::dumpenv(path, &current_env()) {
            Ok(()) => process::exit(0),
            Err(DumpError::Locked) => process::exit(1),
            Err(DumpError::Io(e)) => {
                eprintln!(
                    "failed to write {}: {}",
                    path.display(),
                    strerror(&e)
                );
                process::exit(-1);
            }
        }
    }

    // --readenv: start in the recorded environment. The C tool re-executed
    // itself in a forked child for this; here it is just the base environment.
    let base_env = match &opts.envfile {
        Some(f) => envfile::readenv(Path::new(f)).unwrap_or_else(|e| {
            err_quit(&format!(
                    "failed to read environment from {}: {}",
                    Path::new(f).display(),
                    strerror(&e)
                ),
            )
        }),
        None => current_env(),
    };

    let program = &args[nopt..];
    if program.is_empty() {
        err_quit(&usage(&progname));
    }
    let plan = ChildPlan::new(&opts, program, base_env)
        .unwrap_or_else(|e| err_quit(&format!("bad argument: {e}")));

    install_sigcont_handler();

    if opts.redirect_error {
        // SAFETY: dup2 on our stdio
        unsafe { libc::dup2(1, 2) };
    }

    let mut master = -1;
    // SAFETY: we are single-threaded; the children only use the prepared plan
    // and exit through exec or _exit
    let pid = unsafe {
        if opts.nopty {
            let pid = libc::fork();
            if pid == 0 {
                // a session of its own, so the whole program can be signalled,
                // unless our stdout is a terminal, which it would lose as its
                // controlling terminal (the C tool also did setsid in the parent)
                if libc::isatty(1) == 0 {
                    libc::setsid();
                }
                run_child(&plan);
            }
            pid
        } else if let Some(pts) = &plan.pty {
            let pid = libc::fork();
            if pid == 0 {
                if libc::setsid() < 0 {
                    child_fail(1,
                        "setsid error",
                        Some(io::Error::last_os_error()),
                    );
                }
                let fd = libc::open(pts.as_ptr(), libc::O_RDWR);
                if fd == -1 {
                    let e = io::Error::last_os_error();
                    let msg = format!(
                        "ERROR cannot open pty \"{}\" -- {}\n",
                        pts.to_string_lossy(),
                        strerror(&e)
                    );
                    child_fail(1, &msg, Some(e));
                }
                attach_terminal(fd);
                run_child(&plan);
            }
            pid
        } else {
            // carry our terminal's settings over to the new one, if we have one
            let mut tio: Option<(libc::termios, libc::winsize)> = None;
            if libc::isatty(0) == 1 {
                let mut t: libc::termios = std::mem::zeroed();
                let mut w: libc::winsize = std::mem::zeroed();
                if libc::tcgetattr(0, &mut t) == -1 {
                    err_sys("tcgetattr failed");
                }
                if libc::ioctl(0, libc::TIOCGWINSZ, &mut w) == -1 {
                    err_sys("ioctl(TIOCGWINSZ) failed");
                }
                tio = Some((t, w));
            }
            let (fd, name) = open_master();
            master = fd;
            let pid = libc::fork();
            if pid == 0 {
                if libc::setsid() < 0 {
                    child_fail(1,
                        "setsid error",
                        Some(io::Error::last_os_error()),
                    );
                }
                // no O_NOCTTY: as a session leader we get it as controlling terminal
                let slave = libc::open(name.as_ptr(), libc::O_RDWR);
                if slave == -1 {
                    child_fail(1,
                        "can't open slave pty",
                        Some(io::Error::last_os_error()),
                    );
                }
                if let Some((t, w)) = &tio {
                    if libc::tcsetattr(slave, libc::TCSANOW, t) == -1 {
                        child_fail(1,
                            "tcsetattr(TCSANOW) failed",
                            Some(io::Error::last_os_error()),
                        );
                    }
                    if libc::ioctl(slave, libc::TIOCSWINSZ, w) == -1 {
                        child_fail(1,
                            "ioctl(TIOCSWINSZ) failed",
                            Some(io::Error::last_os_error()),
                        );
                    }
                }
                libc::close(master);
                attach_terminal(slave);
                run_child(&plan);
            }
            pid
        }
    };
    if pid < 0 {
        err_sys("fork error");
    }

    // ignore terminal-generated SIGINT/SIGQUIT, so we live to report the
    // program's exit status
    set_signal(libc::SIGINT, libc::SIG_IGN);
    set_signal(libc::SIGQUIT, libc::SIG_IGN);

    let waited = if master >= 0 {
        let ended = relay(0, master, 1).unwrap_or_else(|e| {
            eprintln!("relay failed: {}", strerror(&e));
            Ended::Hangup
        });
        match ended {
            Ended::ChildClosed => wait_child(pid),
            Ended::Hangup => {
                // the IDE is gone: hang up the terminal and end the program
                // SAFETY: closing our master fd
                unsafe { libc::close(master) };
                terminate(pid)
            }
        }
    } else {
        wait_child(pid)
    };
    let end = monotonic_ms();

    let Some((status, ru)) = waited else {
        process::exit(1)
    };

    if let Some(r) = &opts.reportfile {
        let mut path = r.clone();
        path.push(format!(".{}", process::id()));
        let start = start_ms();
        let outcome = Outcome {
            status,
            rtime_ms: if start == 0 { 0 } else { end - start },
            utime_ms: rusage_ms(ru.ru_utime),
            stime_ms: rusage_ms(ru.ru_stime),
            maxrss_kb: maxrss_kb(&ru),
        };
        let written = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .and_then(|mut f| f.write_all(format(&outcome).as_bytes()));
        if let Err(e) = written {
            eprintln!(
                "Failed to write status to {}: {}",
                String::from_utf8_lossy(path.as_bytes()),
                strerror(&e)
            );
        }
    }

    process::exit(exit_code(status));
}
