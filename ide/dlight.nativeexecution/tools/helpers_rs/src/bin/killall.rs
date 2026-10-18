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

use nativeexecution_helpers::killall::{pfind, readopts, sendsignal, sigqueue, OptError, Scope};
use nativeexecution_helpers::{atoi, err_quit};
use std::path::Path;
use std::process;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let progname = args
        .first()
        .and_then(|a| Path::new(a).file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("killall")
        .to_string();

    let (opts, nopt) = match readopts(&args) {
        Ok(r) => r,
        Err(OptError::Unrecognized(opt)) => {
            println!("ERROR unrecognized option '{opt}'");
            process::exit(-1);
        }
        Err(OptError::Quit(msg)) => err_quit(&progname, &msg),
    };
    let operands = &args[nopt..];

    if operands.is_empty() {
        err_quit(
            &progname,
            &format!(
                "\n\nusage: {p} -p|-g|-s signal_name id\n\
                 \t-p\t\tsend signal signal_name to a process with the specified id\n\
                 \t-g\t\tsend signal signal_name to all processes with the specified process group ID\n\
                 \t-s\t\tsend signal signal_name to all processes with the specified session ID\n\
                 \nusage: {p} -q signal_name pid value\n\
                 \t-q\t\tsignal process with the given signal and integer value attached.\n\
                 \nusage: {p} [-n] -e signal_name env\n\
                 \t-e\t\tfind all processes that have env entry in their environment and send the signal to them\n",
                p = progname
            ),
        );
    }

    if opts.scope == Scope::Env {
        for pid in pfind(&operands[0]) {
            if opts.nosignal {
                println!("{pid}");
            } else {
                sendsignal(Scope::Pid, pid, opts.sig);
            }
        }
        process::exit(0);
    }

    let id = atoi(&operands[0]);
    if id <= 0 {
        err_quit(&progname, &format!("Wrong ID: {}", operands[0]));
    }

    let rc = if opts.scope == Scope::Queue {
        let Some(value) = operands.get(1) else {
            err_quit(&progname, "value is expected for sigqueue");
        };
        sigqueue(id, opts.sig, atoi(value))
    } else {
        sendsignal(opts.scope, id, opts.sig)
    };
    process::exit(rc);
}
