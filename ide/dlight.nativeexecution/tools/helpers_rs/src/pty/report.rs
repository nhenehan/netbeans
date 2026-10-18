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

//! The `--report` file (do_report in tools/pty/src/pty.c), parsed by
//! ProcessStatusEx and shown by PostMessageDisplayer ("RUN FINISHED; ...").

/// glibc's strsignal() wording. The binaries are static musl builds, whose
/// strsignal words some signals differently, and the text ends up in the IDE.
pub fn signal_name(sig: i32) -> String {
    let s = match sig {
        1 => "Hangup",
        2 => "Interrupt",
        3 => "Quit",
        4 => "Illegal instruction",
        5 => "Trace/breakpoint trap",
        6 => "Aborted",
        7 => "Bus error",
        8 => "Floating point exception",
        9 => "Killed",
        10 => "User defined signal 1",
        11 => "Segmentation fault",
        12 => "User defined signal 2",
        13 => "Broken pipe",
        14 => "Alarm clock",
        15 => "Terminated",
        16 => "Stack fault",
        17 => "Child exited",
        18 => "Continued",
        19 => "Stopped (signal)",
        20 => "Stopped",
        21 => "Stopped (tty input)",
        22 => "Stopped (tty output)",
        23 => "Urgent I/O condition",
        24 => "CPU time limit exceeded",
        25 => "File size limit exceeded",
        26 => "Virtual timer expired",
        27 => "Profiling timer expired",
        28 => "Window changed",
        29 => "I/O possible",
        30 => "Power failure",
        31 => "Bad system call",
        _ => {
            let (min, max) = (libc::SIGRTMIN(), libc::SIGRTMAX());
            return if (min..=max).contains(&sig) {
                format!("Real-time signal {}", sig - min)
            } else {
                format!("Unknown signal {sig}")
            };
        }
    };
    s.to_string()
}

/// What happened to the child, for the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    /// waitpid status
    pub status: i32,
    /// ms from the child's exec to its end, or 0 if the start was not seen
    pub rtime_ms: i64,
    /// children's user and system time in ms, max RSS in KB (getrusage)
    pub utime_ms: i64,
    pub stime_ms: i64,
    pub maxrss_kb: i64,
}

/// The report text. The C tool printed UTIME/STIME in seconds when it fell
/// back to getrusage; here they are always milliseconds, as ProcessStatusEx
/// expects.
pub fn format(o: &Outcome) -> String {
    let st = o.status;
    let rc = if libc::WIFEXITED(st) {
        libc::WEXITSTATUS(st)
    } else {
        1
    };
    let sig = if libc::WIFSIGNALED(st) {
        signal_name(libc::WTERMSIG(st))
    } else {
        "-".into()
    };
    let core = i32::from(libc::WIFSIGNALED(st) && libc::WCOREDUMP(st));
    format!(
        "RC: {rc}\nSIG: {sig}\nCORE: {core}\nRTIME: {}\nUTIME: {}\nSTIME: {}\nMAXRSS: {}\n",
        o.rtime_ms, o.utime_ms, o.stime_ms, o.maxrss_kb
    )
}

/// The exit code pty passes on: the child's, or 1 if it did not exit normally.
pub fn exit_code(status: i32) -> i32 {
    if libc::WIFEXITED(status) {
        libc::WEXITSTATUS(status)
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exited() {
        let o = Outcome {
            status: 3 << 8,
            rtime_ms: 12,
            utime_ms: 4,
            stime_ms: 5,
            maxrss_kb: 6,
        };
        assert_eq!(
            format(&o),
            "RC: 3\nSIG: -\nCORE: 0\nRTIME: 12\nUTIME: 4\nSTIME: 5\nMAXRSS: 6\n"
        );
        assert_eq!(exit_code(o.status), 3);
    }

    #[test]
    fn signalled() {
        let o = Outcome {
            status: libc::SIGSEGV | 0x80,
            rtime_ms: 0,
            utime_ms: 0,
            stime_ms: 0,
            maxrss_kb: 0,
        };
        let r = format(&o);
        assert!(
            r.starts_with("RC: 1\nSIG: Segmentation fault\nCORE: 1\n"),
            "{r}"
        );
        assert_eq!(exit_code(o.status), 1);
    }

    #[cfg(target_env = "gnu")]
    #[test]
    fn same_wording_as_glibc() {
        for sig in 1..=libc::SIGRTMAX() {
            // SAFETY: strsignal returns a valid C string for any int
            let c = unsafe { std::ffi::CStr::from_ptr(libc::strsignal(sig)) };
            assert_eq!(signal_name(sig), c.to_str().unwrap(), "signal {sig}");
        }
    }
}
