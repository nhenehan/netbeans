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

//! Code shared by the nativeexecution helper binaries.

pub mod killall;
pub mod pty;
pub mod pty_open;

use std::io::Write;
use std::process;

/// The C tools' `err_quit`: prints `<progname>: <msg>` to stderr and exits with 1.
pub fn err_quit(progname: &str, msg: &str) -> ! {
    let _ = std::io::stdout().flush();
    eprintln!("{progname}: {msg}");
    process::exit(1);
}

/// C `atoi`: optional leading whitespace and sign, then digits; anything else
/// ends the number, and no digits gives 0. Saturates instead of overflowing.
pub fn atoi(s: &str) -> i32 {
    let s = s.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let (neg, digits) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let mut n: i64 = 0;
    for b in digits.bytes().take_while(u8::is_ascii_digit) {
        n = (n * 10 + i64::from(b - b'0')).min(i64::from(i32::MAX) + 1);
    }
    let n = if neg { -n } else { n };
    n.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

#[cfg(test)]
mod tests {
    use super::atoi;

    #[test]
    fn atoi_matches_c() {
        assert_eq!(atoi("42"), 42);
        assert_eq!(atoi("  -7x"), -7);
        assert_eq!(atoi("+3"), 3);
        assert_eq!(atoi("abc"), 0);
        assert_eq!(atoi(""), 0);
        assert_eq!(atoi("99999999999"), i32::MAX);
        assert_eq!(atoi("-99999999999"), i32::MIN);
    }
}
