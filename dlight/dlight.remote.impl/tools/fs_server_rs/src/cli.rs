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

//! Command line, compatible with the C server's `getopt("r:pv:t:lsd:cR:K:e:")`
//! and with what `FSSDispatcher.FsServer` actually passes:
//!
//! ```text
//! -t 4 -p -d <cache> [-r <sec>] [-v <lvl>] [-l] [-e [<file>]] [-c] [-t <n>] [-K <msec>|<pid>:<msec>]
//! ```
//!
//! Options may repeat (the IDE sends `-t` twice when `remote.fs_server.threads`
//! is set); the last occurrence wins, as with getopt.

use clap::error::ErrorKind;
use clap::Parser;

use crate::exitcodes;
use crate::out;
use crate::report_error;

pub const MAX_THREAD_COUNT: usize = 80;
pub const DEFAULT_THREAD_COUNT: usize = 4;

#[derive(Parser, Debug)]
#[command(
    name = "fs_server",
    version,
    about = "Remote file system server for the NetBeans IDE",
    args_override_self = true,
    disable_help_subcommand = true
)]
struct Cli {
    /// Response processing threads count (default 4, max 80)
    #[arg(short = 't', long = "threads", value_name = "NTHREADS")]
    threads: Option<String>,

    /// Log responses into persistence (the cache)
    #[arg(short = 'p', long = "persistence")]
    persistence: bool,

    /// Set refresh ON and set the refresh interval in seconds (requires -p)
    #[arg(short = 'r', long = "refresh", value_name = "NSEC")]
    refresh: Option<String>,

    /// Redirect trace and error messages to FILE (default: <cache>/stderr.txt)
    #[arg(short = 'e', long = "error-log", value_name = "FILE", num_args = 0..=1, default_missing_value = "")]
    error_log: Option<String>,

    /// Refresh mode: i - implicit (poll all known dirs), e - explicit (only watched dirs)
    #[arg(short = 'R', long = "refresh-mode", value_name = "i|e")]
    refresh_mode: Option<String>,

    /// Print trace messages: 0 none .. 4 finest
    #[arg(short = 'v', long = "verbose", value_name = "LEVEL")]
    verbose: Option<String>,

    /// Log all requests into <cache>/log
    #[arg(short = 'l', long = "log")]
    log: bool,

    /// Print some statistics to stderr on exit
    #[arg(short = 's', long = "statistics")]
    statistics: bool,

    /// Persistence directory (absolute, or relative to ~/.netbeans/remotefs)
    #[arg(short = 'd', long = "dir", value_name = "DIR")]
    dir: Option<String>,

    /// Clean up persistence upon startup
    #[arg(short = 'c', long = "cleanup")]
    cleanup: bool,

    /// Kill another fs_server that holds the cache lock: SIGTERM, then SIGKILL,
    /// waiting at most MSEC milliseconds for each; with PID only that process is killed
    #[arg(short = 'K', long = "kill-locker", value_name = "[PID:]MSEC")]
    kill: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    pub threads: usize,
    pub persistence: bool,
    pub refresh: bool,
    pub refresh_sleep: u64,
    pub refresh_explicit: bool,
    /// Some(None): -e without a file name
    pub error_log: Option<Option<String>>,
    pub trace_level: Option<i32>,
    pub log: bool,
    pub statistics: bool,
    pub persistence_dir: Option<String>,
    pub clear_persistence: bool,
    /// milliseconds to wait for the locker to die; 0 = don't kill
    pub kill_wait_ms: u64,
    /// 0 = any locker
    pub kill_pid: u32,
}

/// C `atoi()`: leading blanks, optional sign, digits; anything else stops; 0 if none.
pub fn atoi(s: &str) -> i64 {
    let s = s.trim_start();
    let (neg, digits) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let mut v: i64 = 0;
    for c in digits.bytes().take_while(u8::is_ascii_digit) {
        v = v.saturating_mul(10).saturating_add(i64::from(c - b'0'));
    }
    if neg {
        -v
    } else {
        v
    }
}

pub fn usage() -> String {
    format!(
        "fs_server {ver} (rust)\n\
         Usage: fs_server [-t nthreads] [-v] [-p] [-r]\n\
         \x20  -t <nthreads> response processing threads count (default is {DEFAULT_THREAD_COUNT})\n\
         \x20  -p log responses into persistence\n\
         \x20  -r <nsec>  set refresh ON and sets refresh interval in seconds\n\
         \x20  -e [<file>]  redirect trace and error messages to file; by default file is ${{cache}}/stderr.txt\n\
         \x20  -R <i|e>  refresh mode: i - implicit, e - explicit\n\
         \x20  -v <verbose-level>: print trace messages\n\
         \x20  -l log all requests into log file\n\
         \x20  -s statistics: print some statistics output to stderr\n\
         \x20  -d persistence directory: where to log responses (valid only if -p is set)\n\
         \x20  -c cleanup persistence upon startup\n\
         \x20  -K <PID>:<msec>|<msec> kill another fs_server process that locks the cache via sending SIGTERM and\n\
         \x20     wait maximum <msec> milliseconds until it releases the lock\n\
         \x20     if <PID> is specified then only the process with this PID can be killed\n",
        ver = env!("CARGO_PKG_VERSION")
    )
}

fn wrong_argument(msg: &str) -> ! {
    report_error!("{msg}");
    out::err_print(&usage());
    std::process::exit(exitcodes::WRONG_ARGUMENT);
}

pub fn parse<I, T>(args: I) -> Options
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(e) if matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) => {
            e.exit()
        }
        Err(e) => wrong_argument(&format!("{}\n", e.render().to_string().trim_end())),
    };
    from_cli(cli)
}

fn from_cli(cli: Cli) -> Options {
    let mut threads = DEFAULT_THREAD_COUNT;
    if let Some(t) = &cli.threads {
        let n = atoi(t);
        if n > 0 {
            if n as usize > MAX_THREAD_COUNT {
                report_error!(
                    "incorrect value of -t flag: {n}. Should not exceed {MAX_THREAD_COUNT}.\n"
                );
                threads = MAX_THREAD_COUNT;
            } else {
                threads = n as usize;
            }
        }
    }

    let mut refresh_sleep = 1;
    if let Some(r) = &cli.refresh {
        let n = atoi(r);
        if n >= 0 {
            refresh_sleep = n as u64;
        }
    }

    let refresh_explicit = match cli.refresh_mode.as_deref().map(str::as_bytes) {
        None => false,
        Some([b'i', ..]) => false,
        Some([b'e', ..]) => true,
        Some(_) => wrong_argument(&format!(
            "incorrect value of -R flag: {}\n",
            cli.refresh_mode.as_deref().unwrap_or_default()
        )),
    };

    let trace_level = cli.verbose.as_deref().map(|v| {
        let n = atoi(v) as i32;
        if !(out::TRACE_NONE..=out::TRACE_FINEST).contains(&n) {
            report_error!(
                "incorrect value of -v flag: {n}. Defaulting to {}\n",
                if n > out::TRACE_FINEST {
                    out::TRACE_FINEST
                } else {
                    out::TRACE_INFO
                }
            );
        }
        n
    });

    // "<msec>" or "<pid>:<msec>" (the C parser always took the first number as the PID)
    let (kill_pid, kill_wait_ms) = match cli.kill.as_deref() {
        None => (0, 0),
        Some(spec) => match spec.split_once(':') {
            Some((pid, ms)) => (atoi(pid).max(0) as u32, atoi(ms).max(0) as u64),
            None => (0, atoi(spec).max(0) as u64),
        },
    };

    let error_log = cli
        .error_log
        .map(|f| if f.is_empty() { None } else { Some(f) });

    let opts = Options {
        threads,
        persistence: cli.persistence,
        refresh: cli.refresh.is_some(),
        refresh_sleep,
        refresh_explicit,
        error_log,
        trace_level,
        log: cli.log,
        statistics: cli.statistics,
        persistence_dir: cli.dir,
        clear_persistence: cli.cleanup,
        kill_wait_ms,
        kill_pid,
    };
    if opts.refresh && !opts.persistence {
        wrong_argument(
            "incorrect parameters combination: refresh without persistence does not work\n",
        );
    }
    opts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Options {
        let mut v = vec!["fs_server"];
        v.extend_from_slice(args);
        from_cli(Cli::try_parse_from(v).expect("parse"))
    }

    #[test]
    fn default_ide_invocation() {
        // FSSDispatcher: -t 4 -p -d <cache>
        let o = p(&["-t", "4", "-p", "-d", "/home/u/.netbeans/remotefs/x"]);
        assert_eq!(o.threads, 4);
        assert!(o.persistence);
        assert!(!o.refresh);
        assert_eq!(
            o.persistence_dir.as_deref(),
            Some("/home/u/.netbeans/remotefs/x")
        );
        assert_eq!(o.error_log, None);
        assert_eq!(o.kill_wait_ms, 0);
    }

    #[test]
    fn full_ide_invocation() {
        let o = p(&[
            "-t",
            "4",
            "-p",
            "-d",
            "/c",
            "-r",
            "5",
            "-v",
            "3",
            "-l",
            "-e",
            "-c",
            "-t",
            "16",
            "-K",
            "1234:3000",
        ]);
        assert_eq!(o.threads, 16, "last -t wins");
        assert!(o.refresh);
        assert_eq!(o.refresh_sleep, 5);
        assert_eq!(o.trace_level, Some(3));
        assert!(o.log);
        assert_eq!(
            o.error_log,
            Some(None),
            "-e followed by an option has no file"
        );
        assert!(o.clear_persistence, "-c after a bare -e is still an option");
        assert_eq!((o.kill_pid, o.kill_wait_ms), (1234, 3000));
    }

    #[test]
    fn error_file_and_kill_all() {
        let o = p(&["-p", "-e", "/tmp/err.txt", "-K", "3000"]);
        assert_eq!(o.error_log, Some(Some("/tmp/err.txt".into())));
        assert_eq!((o.kill_pid, o.kill_wait_ms), (0, 3000));
    }

    #[test]
    fn threads_clamped_and_ignored() {
        assert_eq!(p(&["-t", "500"]).threads, MAX_THREAD_COUNT);
        assert_eq!(p(&["-t", "0"]).threads, DEFAULT_THREAD_COUNT);
        assert_eq!(p(&["-t", "x"]).threads, DEFAULT_THREAD_COUNT);
    }

    #[test]
    fn refresh_mode() {
        assert!(p(&["-R", "e"]).refresh_explicit);
        assert!(!p(&["-R", "implicit"]).refresh_explicit);
    }

    #[test]
    fn attached_values() {
        let o = p(&["-t8", "-pd/c"]);
        assert_eq!(o.threads, 8);
        assert!(o.persistence);
        assert_eq!(o.persistence_dir.as_deref(), Some("/c"));
    }

    #[test]
    fn atoi_like_c() {
        assert_eq!(atoi(" 42abc"), 42);
        assert_eq!(atoi("-7"), -7);
        assert_eq!(atoi("abc"), 0);
    }
}
