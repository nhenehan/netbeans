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

//! Command line of pty (tools/pty/src/options.c), as NbNativeProcess builds it.

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// `--no-pty`: run the program on our own stdio
    pub nopty: bool,
    /// `--set-erase-key`: VERASE = ^H on the terminal
    pub set_erase_key: bool,
    /// `--redirect-error`: stderr goes to stdout
    pub redirect_error: bool,
    /// `--report FILE`: write exit information to `FILE.<pid>`
    pub reportfile: Option<OsString>,
    /// `--readenv FILE` (start in the recorded environment) or `--dumpenv FILE`
    pub envfile: Option<OsString>,
    /// `--dumpenv FILE`: record the environment and exit
    pub dumpenv: bool,
    /// `-p PTS`: use this terminal instead of allocating one
    pub pty: Option<OsString>,
    /// `--dir DIR`
    pub wdir: Option<OsString>,
    /// `--env NAME=VALUE`, in order
    pub envvars: Vec<OsString>,
    /// `-e`: echo off
    pub noecho: bool,
    /// `-w`: wait for SIGCONT before exec
    pub wait_signal: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum OptError {
    /// stdout `ERROR unrecognized option '<opt>'`, exit 255
    Unrecognized(String),
    /// err_quit message, exit 1
    Quit(&'static str),
}

/// Parses the options in `args` (`args[0]` is the program name) and returns
/// them with the index of the program to run. Parsing stops at the first
/// argument that doesn't start with `-`; `--dumpenv` ends it at once.
pub fn readopts(args: &[OsString]) -> Result<(Options, usize), OptError> {
    let mut o = Options::default();
    let mut idx = 1;
    // the C messages, including its "-report" typo and trailing "\n"
    let value = |idx: usize, msg: &'static str| -> Result<OsString, OptError> {
        match args.get(idx) {
            Some(v) if !v.is_empty() => Ok(v.clone()),
            _ => Err(OptError::Quit(msg)),
        }
    };
    while idx < args.len() && args[idx].as_bytes().first() == Some(&b'-') {
        match args[idx].as_bytes() {
            b"--no-pty" => o.nopty = true,
            b"--set-erase-key" => o.set_erase_key = true,
            b"--redirect-error" => o.redirect_error = true,
            b"-e" => o.noecho = true,
            b"-w" => o.wait_signal = true,
            b"--report" => {
                idx += 1;
                o.reportfile = Some(value(idx, "missing envfile after -report\n")?);
            }
            b"--dumpenv" => {
                o.envfile = Some(value(idx + 1, "missing envfile after --dumpenv\n")?);
                o.dumpenv = true;
                return Ok((o, args.len()));
            }
            // what the C tool's re-exec turned "--readenv FILE" into; skips FILE
            b"--" => idx += 1,
            b"--readenv" => {
                idx += 1;
                o.envfile = Some(value(idx, "missing envfile after --readenv\n")?);
            }
            b"-p" => {
                idx += 1;
                o.pty = Some(value(idx, "missing pty after -p\n")?);
            }
            b"--dir" => {
                idx += 1;
                o.wdir = Some(value(idx, "missing dir after --dir\n")?);
            }
            b"--env" => {
                idx += 1;
                o.envvars
                    .push(value(idx, "missing variable=value pair after --env\n")?);
            }
            other => {
                return Err(OptError::Unrecognized(
                    String::from_utf8_lossy(other).into_owned(),
                ))
            }
        }
        idx += 1;
    }
    Ok((o, idx))
}

pub fn usage(progname: &str) -> String {
    format!(
        "\n\n\
         usage: {p} [-e] [--no-pty] [-w] [-p pts_name] [--set-erase-key]\n\
         \t\t[--readenv env_file] [[--env NAME=VALUE] ...] [--dir dir]\n\
         \t\t[--redirect-error] [--report report_file] program [ arg ... ]\n\n\
         \t-e\t\t turn echoing off\n\
         \t-w\t\t wait SIGCONT after reporting PID/TTY/.. but before executing the program\n\
         \t-p\t\t define pts_name to attach process's I/O instead of opening a new one\n\
         \t--readenv\t read environment to start process in from a file\n\
         \t--env\t\t pass (additional) environment variable to the process\n\
         \t--dir\t\t change working directory for starting process\n\
         \t--redirect-error redirect stderror to stdout for starting process (makes sense if --no-pty only)\n\
         \t--report\t record process' and exit status information into specified file\n\n\
         usage: {p} --dumpenv env_file\n\
         \t--dumpenv\t dump environment to a file\n",
        p = progname
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<OsString> {
        v.iter().map(OsString::from).collect()
    }

    #[test]
    fn nbnativeprocess_command_line() {
        let a = args(&[
            "pty",
            "--dir",
            "/w",
            "--set-erase-key",
            "-p",
            "/dev/pts/3",
            "--report",
            "/t/status",
            "--readenv",
            "/t/env",
            "--env",
            "A=1",
            "--env",
            "B=2",
            "/bin/sh",
            "-c",
            "exec x",
        ]);
        let (o, n) = readopts(&a).unwrap();
        assert_eq!(n, 14);
        assert_eq!(a[n], "/bin/sh");
        assert_eq!(o.wdir.as_deref(), Some("/w".as_ref()));
        assert!(o.set_erase_key && !o.nopty && !o.dumpenv);
        assert_eq!(o.pty.as_deref(), Some("/dev/pts/3".as_ref()));
        assert_eq!(o.reportfile.as_deref(), Some("/t/status".as_ref()));
        assert_eq!(o.envfile.as_deref(), Some("/t/env".as_ref()));
        assert_eq!(o.envvars, args(&["A=1", "B=2"]));
    }

    #[test]
    fn no_pty_flags() {
        let (o, n) = readopts(&args(&[
            "pty",
            "--no-pty",
            "--redirect-error",
            "-w",
            "-e",
            "ls",
        ]))
        .unwrap();
        assert!(o.nopty && o.redirect_error && o.wait_signal && o.noecho);
        assert_eq!(n, 5);
    }

    #[test]
    fn dumpenv_ends_parsing() {
        let (o, n) = readopts(&args(&["pty", "--dumpenv", "/t/env", "junk"])).unwrap();
        assert!(o.dumpenv);
        assert_eq!(o.envfile.as_deref(), Some("/t/env".as_ref()));
        assert_eq!(n, 4);
    }

    #[test]
    fn double_dash_skips_one() {
        let (_, n) = readopts(&args(&["pty", "--", "/t/env", "prog"])).unwrap();
        assert_eq!(n, 3);
    }

    #[test]
    fn errors() {
        assert_eq!(
            readopts(&args(&["pty", "--bogus"])),
            Err(OptError::Unrecognized("--bogus".into()))
        );
        assert_eq!(
            readopts(&args(&["pty", "--dir"])),
            Err(OptError::Quit("missing dir after --dir\n"))
        );
        assert_eq!(
            readopts(&args(&["pty", "--env", ""])),
            Err(OptError::Quit("missing variable=value pair after --env\n"))
        );
    }
}
