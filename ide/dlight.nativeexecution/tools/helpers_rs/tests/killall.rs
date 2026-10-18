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

//! Runs the killall binary against real child processes. Set KILLALL_C to a C
//! killall (e.g. from the exechlp or 24aefa9 zip) to also compare the two on
//! the error paths.

use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const KILLALL: &str = env!("CARGO_BIN_EXE_killall");

fn killall(args: &[&str]) -> Output {
    Command::new(KILLALL).args(args).output().unwrap()
}

/// `sleep 30` in its own process group and session (setsid), so -g and -s only
/// reach it.
fn sleeper(env: Option<(&str, &str)>) -> Child {
    let mut cmd = Command::new("sleep");
    cmd.arg("30").stdout(Stdio::null());
    if let Some((k, v)) = env {
        cmd.env(k, v);
    }
    // SAFETY: setsid is async-signal-safe
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    cmd.spawn().unwrap()
}

/// Waits for the child and returns the signal that ended it.
fn killed_by(mut child: Child) -> Option<i32> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(st) = child.try_wait().unwrap() {
            return st.signal();
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn pid(c: &Child) -> String {
    c.id().to_string()
}

#[test]
fn by_pid() {
    let c = sleeper(None);
    let out = killall(&["-p", "TERM", &pid(&c)]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(killed_by(c), Some(libc::SIGTERM));
}

#[test]
fn by_group() {
    let c = sleeper(None);
    assert_eq!(killall(&["-g", "KILL", &pid(&c)]).status.code(), Some(0));
    assert_eq!(killed_by(c), Some(libc::SIGKILL));
}

#[test]
fn by_session() {
    let c = sleeper(None);
    // the child calls setsid in pre_exec; give it a moment to be visible as leader
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(killall(&["-s", "INT", &pid(&c)]).status.code(), Some(0));
    assert_eq!(killed_by(c), Some(libc::SIGINT));
}

#[test]
fn sigqueue_delivers() {
    let c = sleeper(None);
    assert_eq!(
        killall(&["-q", "USR1", &pid(&c), "42"]).status.code(),
        Some(0)
    );
    assert_eq!(killed_by(c), Some(libc::SIGUSR1));
}

#[test]
fn by_env_lists_then_signals() {
    let marker = format!("NBMAGIC_TEST={}", std::process::id());
    let (k, v) = marker.split_once('=').unwrap();
    let c = sleeper(Some((k, v)));
    std::thread::sleep(Duration::from_millis(50));

    let out = killall(&["-n", "-e", "NULL", &marker]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), pid(&c));

    // prefixes and other values must not match
    let out = killall(&["-n", "-e", "NULL", &marker[..marker.len() - 1]]);
    assert!(out.stdout.is_empty());

    assert_eq!(killall(&["-e", "TERM", &marker]).status.code(), Some(0));
    assert_eq!(killed_by(c), Some(libc::SIGTERM));
}

#[test]
fn env_marker_past_4k() {
    // the C tool only read the first 4048 bytes of /proc/<pid>/environ
    let marker = format!("NBMAGIC_LATE={}", std::process::id());
    let (k, v) = marker.split_once('=').unwrap();
    let mut cmd = Command::new("sleep");
    cmd.arg("30")
        .env_clear()
        .env("AAAA_PAD", "x".repeat(8192))
        .env(k, v);
    let c = cmd.spawn().unwrap();
    std::thread::sleep(Duration::from_millis(50));
    let out = killall(&["-n", "-e", "NULL", &marker]);
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), pid(&c));
    killall(&["-p", "KILL", &pid(&c)]);
    assert_eq!(killed_by(c), Some(libc::SIGKILL));
}

#[test]
fn null_signal_probes() {
    let c = sleeper(None);
    assert_eq!(killall(&["-p", "NULL", &pid(&c)]).status.code(), Some(0));
    killall(&["-p", "KILL", &pid(&c)]);
    killed_by(c);
}

/// (args, exit code, stdout, stderr) for the error paths
fn error_cases() -> Vec<(Vec<&'static str>, i32)> {
    vec![
        (vec![], 1),
        (vec!["-p", "TERM"], 1),
        (vec!["-p"], 1),
        (vec!["-p", "BOGUS", "1"], 1),
        (vec!["-x", "TERM", "1"], 255),
        (vec!["-p", "TERM", "0"], 1),
        (vec!["-p", "TERM", "abc"], 1),
        (vec!["-q", "TERM", "12345"], 1),
        // pid 2^22+1 is above the default pid_max, so kill fails with ESRCH
        (vec!["-p", "NULL", "4194305"], 255),
        (vec!["-n", "-e", "NULL", "NO_SUCH_MARKER=1"], 0),
    ]
}

#[test]
fn error_paths() {
    for (args, code) in error_cases() {
        let out = killall(&args);
        assert_eq!(out.status.code(), Some(code), "{args:?}: {out:?}");
    }
    let out = killall(&["-x"]);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "ERROR unrecognized option '-x'\n"
    );
    let out = killall(&["-p", "BOGUS", "1"]);
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "killall: Unknown signal BOGUS\n\n"
    );
}

#[test]
fn same_as_c_on_error_paths() {
    let Some(c_killall) = std::env::var_os("KILLALL_C") else {
        eprintln!("KILLALL_C not set; skipping the comparison with the C killall");
        return;
    };
    for (args, _) in error_cases() {
        let rs = killall(&args);
        let c = Command::new(&c_killall).args(&args).output().unwrap();
        assert_eq!(rs.status.code(), c.status.code(), "exit code for {args:?}");
        assert_eq!(rs.stdout, c.stdout, "stdout for {args:?}");
        assert_eq!(rs.stderr, c.stderr, "stderr for {args:?}");
    }
}
