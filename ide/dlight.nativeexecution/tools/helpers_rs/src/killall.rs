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

//! killall: sends a signal to a process, process group, session, or to every
//! process whose environment holds a marker entry. Port of
//! `tools/killall/src` (Linux); the command line, output and exit codes are the
//! ones `NbKillAllSignalSupport` relies on.

use std::fs;

/// What the signal is sent to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// `-p`: a process id
    Pid,
    /// `-g`: a process group id
    Pgid,
    /// `-s`: a session id
    Sid,
    /// `-e`: every process with this exact `NAME=VALUE` in its environment
    Env,
    /// `-q`: sigqueue to a pid, with an integer value
    Queue,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Options {
    pub scope: Scope,
    pub sig: i32,
    /// `-n`: with `-e`, print the matching pids instead of signalling them
    pub nosignal: bool,
}

/// How option parsing can fail; `main` turns these into the C tool's messages.
#[derive(Debug, PartialEq, Eq)]
pub enum OptError {
    /// Printed to stdout as `ERROR unrecognized option '<opt>'`, exit 255
    Unrecognized(String),
    /// err_quit message (exit 1)
    Quit(String),
}

/// The Linux signal names the C tool knew (its table, filtered by what Linux
/// defines). Signal.java sends the enum name without the `SIG` prefix.
const SIGNALS: &[(&str, i32)] = &[
    ("ABRT", libc::SIGABRT),
    ("ALRM", libc::SIGALRM),
    ("BUS", libc::SIGBUS),
    ("CHLD", libc::SIGCHLD),
    ("CLD", libc::SIGCHLD),
    ("CONT", libc::SIGCONT),
    ("FPE", libc::SIGFPE),
    ("HUP", libc::SIGHUP),
    ("ILL", libc::SIGILL),
    ("INT", libc::SIGINT),
    ("IO", libc::SIGIO),
    ("IOT", libc::SIGIOT),
    ("KILL", libc::SIGKILL),
    ("PIPE", libc::SIGPIPE),
    ("POLL", libc::SIGPOLL),
    ("PROF", libc::SIGPROF),
    ("PWR", libc::SIGPWR),
    ("QUIT", libc::SIGQUIT),
    ("SEGV", libc::SIGSEGV),
    ("STKFLT", libc::SIGSTKFLT),
    ("STOP", libc::SIGSTOP),
    ("SYS", libc::SIGSYS),
    ("TERM", libc::SIGTERM),
    ("TRAP", libc::SIGTRAP),
    ("TSTP", libc::SIGTSTP),
    ("TTIN", libc::SIGTTIN),
    ("TTOU", libc::SIGTTOU),
    ("URG", libc::SIGURG),
    ("USR1", libc::SIGUSR1),
    ("USR2", libc::SIGUSR2),
    ("VTALRM", libc::SIGVTALRM),
    ("WINCH", libc::SIGWINCH),
    ("XCPU", libc::SIGXCPU),
    ("XFSZ", libc::SIGXFSZ),
];

/// `NULL` is signal 0 (existence check); otherwise a name from [`SIGNALS`].
pub fn str2sig(name: &str) -> Option<i32> {
    if name == "NULL" {
        return Some(0);
    }
    SIGNALS.iter().find(|(n, _)| *n == name).map(|&(_, s)| s)
}

/// Parses the leading options (`args[0]` is the program name). Returns the
/// options and the index of the first operand. Like the C tool, a later scope
/// option overrides an earlier one and parsing stops at the first non-option.
pub fn readopts(args: &[String]) -> Result<(Options, usize), OptError> {
    let mut opts = Options {
        scope: Scope::Pid,
        sig: 0,
        nosignal: false,
    };
    let mut idx = 1;
    while idx < args.len() && args[idx].starts_with('-') {
        let opt = args[idx].as_str();
        let scope = match opt {
            "-p" => Scope::Pid,
            "-g" => Scope::Pgid,
            "-s" => Scope::Sid,
            "-e" => Scope::Env,
            "-q" => Scope::Queue,
            "-n" => {
                opts.nosignal = true;
                idx += 1;
                continue;
            }
            _ => return Err(OptError::Unrecognized(opt.to_string())),
        };
        opts.scope = scope;
        let sigstr = args.get(idx + 1).map(String::as_str).unwrap_or("");
        if sigstr.is_empty() {
            // the C format strings end in "\n", and err_quit adds another
            return Err(OptError::Quit(format!("missing signal after {opt}\n")));
        }
        opts.sig =
            str2sig(sigstr).ok_or_else(|| OptError::Quit(format!("Unknown signal {sigstr}\n")))?;
        idx += 2;
    }
    Ok((opts, idx))
}

/// Pids (from `/proc`) whose environment has `magicenv` as a whole entry.
/// Unlike the C tool, which read only the first 4048 bytes, this reads the
/// whole environment.
pub fn pfind(magicenv: &str) -> Vec<i32> {
    let needle = magicenv.as_bytes();
    proc_pids()
        .filter(|pid| {
            fs::read(format!("/proc/{pid}/environ"))
                .map(|env| env.split(|&b| b == 0).any(|entry| entry == needle))
                .unwrap_or(false)
        })
        .collect()
}

/// Pids in session `sid`, from field 6 of `/proc/<pid>/stat`.
pub fn session_members(sid: i32) -> Vec<i32> {
    proc_pids()
        .filter(|pid| {
            fs::read_to_string(format!("/proc/{pid}/stat"))
                .ok()
                .and_then(|stat| stat_session(&stat))
                == Some(sid)
        })
        .collect()
}

/// The session id in a `/proc/<pid>/stat` line. The command name (field 2) is in
/// parentheses and may itself contain spaces or `)`, so fields are counted from
/// the last `)`: state, ppid, pgrp, session.
fn stat_session(stat: &str) -> Option<i32> {
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_ascii_whitespace().nth(3)?.parse().ok()
}

fn proc_pids() -> impl Iterator<Item = i32> {
    fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<i32>().ok())
}

/// Sends `sig` to `id` in `scope` (not `Env`). Returns the C call's result:
/// 0, or -1 on failure.
pub fn sendsignal(scope: Scope, id: i32, sig: i32) -> i32 {
    // SAFETY: plain syscalls on integer arguments
    unsafe {
        match scope {
            Scope::Pid => libc::kill(id, sig),
            Scope::Pgid => libc::killpg(id, sig),
            Scope::Sid => {
                // Linux has no sigsend(P_SID); the C tool's -s did nothing
                // defined here. Signal each member; succeed if any was reached.
                let mut rc = -1;
                for pid in session_members(id) {
                    if libc::kill(pid, sig) == 0 {
                        rc = 0;
                    }
                }
                rc
            }
            Scope::Queue | Scope::Env => -1,
        }
    }
}

/// sigqueue(2) with an integer value.
pub fn sigqueue(pid: i32, sig: i32, value: i32) -> i32 {
    // sigval is a union of int and void*; libc models it as the pointer only.
    // Widening the int to pointer size puts it in sival_int on every
    // little-endian target, which covers the Linux targets we build.
    let value = libc::sigval {
        sival_ptr: value as isize as *mut libc::c_void,
    };
    // SAFETY: plain syscall on integer arguments
    unsafe { libc::sigqueue(pid, sig, value) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn signal_names() {
        assert_eq!(str2sig("NULL"), Some(0));
        assert_eq!(str2sig("TERM"), Some(libc::SIGTERM));
        assert_eq!(str2sig("CLD"), Some(libc::SIGCHLD));
        assert_eq!(str2sig("term"), None);
        assert_eq!(str2sig("WAITING"), None); // Solaris-only
    }

    #[test]
    fn options() {
        let (o, n) = readopts(&args(&["killall", "-p", "TERM", "12"])).unwrap();
        assert_eq!(
            (o.scope, o.sig, o.nosignal, n),
            (Scope::Pid, libc::SIGTERM, false, 3)
        );

        let (o, n) = readopts(&args(&["killall", "-n", "-e", "NULL", "A=B"])).unwrap();
        assert_eq!((o.scope, o.sig, o.nosignal, n), (Scope::Env, 0, true, 4));

        let (o, _) = readopts(&args(&["killall", "-p", "INT", "-g", "KILL", "5"])).unwrap();
        assert_eq!((o.scope, o.sig), (Scope::Pgid, libc::SIGKILL));

        assert_eq!(
            readopts(&args(&["killall", "-x"])),
            Err(OptError::Unrecognized("-x".into()))
        );
        assert_eq!(
            readopts(&args(&["killall", "-p"])),
            Err(OptError::Quit("missing signal after -p\n".into()))
        );
        assert_eq!(
            readopts(&args(&["killall", "-q", "BOGUS", "1"])),
            Err(OptError::Quit("Unknown signal BOGUS\n".into()))
        );
    }

    #[test]
    fn stat_session_field() {
        assert_eq!(stat_session("123 (bash) S 1 123 456 34816 ..."), Some(456));
        assert_eq!(stat_session("9 (a) b) c) R 1 2 3 0"), Some(3));
        assert_eq!(stat_session("garbage"), None);
    }

    #[test]
    fn finds_own_session() {
        // SAFETY: getsid(0) only queries the calling process
        let sid = unsafe { libc::getsid(0) };
        assert!(session_members(sid).contains(&(std::process::id() as i32)));
    }
}
