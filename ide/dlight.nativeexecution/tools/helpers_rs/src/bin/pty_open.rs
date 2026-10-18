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

use nativeexecution_helpers::pty_open::{header, open_pty, relay};
use std::fs::File;
use std::io::{self, Write};
use std::mem::ManuallyDrop;
use std::os::fd::FromRawFd;
use std::process;

/// The C tool reports its errors on stdout, where PtyAllocator reads.
fn fail(msg: &str) -> ! {
    let mut out = io::stdout();
    let _ = writeln!(out, "pty_open: {msg}");
    let _ = out.flush();
    process::exit(1);
}

fn main() {
    let mut pty = open_pty().unwrap_or_else(|(what, e)| fail(&format!("{what}: {e}")));

    let mut out = io::stdout().lock();
    if out
        .write_all(header(process::id(), &pty.name).as_bytes())
        .and_then(|_| out.flush())
        .is_err()
    {
        process::exit(1);
    }

    // stdin unbuffered, straight from fd 0: relay polls the fd, so nothing may
    // sit in a userspace buffer. ManuallyDrop leaves fd 0 open.
    // SAFETY: fd 0 is open for the life of the process and only used here
    let mut stdin = ManuallyDrop::new(unsafe { File::from_raw_fd(0) });
    if let Err(msg) = relay(&mut stdin, &mut pty.master, &mut out) {
        drop(out);
        fail(&msg);
    }
    // the slave stays open until here, so the pty outlives every debuggee on it
    drop(pty.slave);
}
