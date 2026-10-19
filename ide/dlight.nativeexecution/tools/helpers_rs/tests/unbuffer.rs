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

//! Preloads unbuffer.so into a program that writes through stdio (GNU sed)
//! and checks that a line shows up on the pipe before the program finishes.
//! The library is built separately (build-dist.sh), so set UNBUFFER_SO to it;
//! UNBUFFER_C can point at a C build to check that one the same way.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

/// How long until sed's first line arrives, with `preload` (or nothing) in
/// LD_PRELOAD. sed prints "one" at once, then waits ~1.5 s for its input.
fn first_line_after(preload: Option<&str>) -> Duration {
    let mut cmd = Command::new("/bin/sh");
    cmd.args(["-c", "(echo one; sleep 1.5; echo two) | sed -n p"])
        .stdout(Stdio::piped());
    if let Some(p) = preload {
        cmd.env("LD_PRELOAD", p);
    }
    let t0 = Instant::now();
    let mut child = cmd.spawn().unwrap();
    let mut out = child.stdout.take().unwrap();
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let mut b = [0u8; 4];
        if out.read_exact(&mut b).is_ok() {
            let _ = tx.send(b);
        }
    });
    let got = rx.recv_timeout(Duration::from_secs(5)).expect("no output");
    let at = t0.elapsed();
    assert_eq!(&got, b"one\n");
    child.wait().unwrap();
    at
}

fn check(so: &str) {
    assert!(
        first_line_after(None) >= Duration::from_millis(1200),
        "sed should block-buffer on a pipe"
    );
    let at = first_line_after(Some(so));
    assert!(
        at < Duration::from_millis(800),
        "with {so}: first line after {at:?}"
    );
}

#[test]
fn makes_stdout_unbuffered() {
    match std::env::var("UNBUFFER_SO") {
        Ok(so) => check(&so),
        Err(_) => eprintln!("UNBUFFER_SO not set; build it with build-dist.sh and point at it"),
    }
}

#[test]
fn c_build_behaves_the_same() {
    if let Ok(so) = std::env::var("UNBUFFER_C") {
        check(&so);
    }
}
