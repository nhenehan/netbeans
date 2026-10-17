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

//! Runtime settings changed by `o` (option) requests.
//!
//! Copy-on-write: each request takes a snapshot (`current()`) and uses it
//! without further locking, like the C `clone_global_settings()`.

use std::sync::{Arc, RwLock};

use crate::out::TRACE_FINER;
use crate::{report_error, trace};

#[derive(Debug, Clone)]
pub struct Settings {
    /// Directories that are never stat'ed; a dummy directory stat is reported instead.
    pub dirs_forbidden_to_stat: Vec<Vec<u8>>,
    /// true: use access(2); false: compute rwx from mode bits, uid and groups.
    pub full_access_check: bool,
}

impl Settings {
    pub fn is_forbidden(&self, path: &[u8]) -> bool {
        self.dirs_forbidden_to_stat.iter().any(|p| p == path)
    }
}

static GLOBAL: RwLock<Option<Arc<Settings>>> = RwLock::new(None);

fn default_settings() -> Arc<Settings> {
    Arc::new(Settings {
        dirs_forbidden_to_stat: Vec::new(),
        full_access_check: true,
    })
}

pub fn current() -> Arc<Settings> {
    let g = GLOBAL.read().unwrap_or_else(|p| p.into_inner());
    g.clone().unwrap_or_else(default_settings)
}

fn change(f: impl FnOnce(&mut Settings)) {
    let mut g = GLOBAL.write().unwrap_or_else(|p| p.into_inner());
    let mut s = g
        .as_deref()
        .cloned()
        .unwrap_or_else(|| (*default_settings()).clone());
    f(&mut s);
    *g = Some(Arc::new(s));
}

pub fn set_full_access_check(full: bool) {
    change(|s| s.full_access_check = full);
}

/// `dir_list` is ':'-separated, exactly as sent by the IDE (may contain empty items).
pub fn set_dirs_forbidden_to_stat(dir_list: &[u8]) {
    let dirs: Vec<Vec<u8>> = dir_list.split(|&c| c == b':').map(<[u8]>::to_vec).collect();
    change(|s| s.dirs_forbidden_to_stat = dirs);
    if crate::out::is_traceable(TRACE_FINER) {
        trace!(TRACE_FINER, "Directories forbidden to stat:\n");
        for d in &current().dirs_forbidden_to_stat {
            trace!(TRACE_FINER, "{}\n", String::from_utf8_lossy(d));
        }
    }
}

fn process_option(key: &[u8], value: &[u8]) {
    match key {
        b"access" => match value {
            b"fast" => set_full_access_check(false),
            b"full" => set_full_access_check(true),
            _ => report_error!(
                "Unexpected option value: {}={}\n",
                String::from_utf8_lossy(key),
                String::from_utf8_lossy(value)
            ),
        },
        b"dirs-forbidden-to-stat" => set_dirs_forbidden_to_stat(value),
        _ => report_error!("Unexpected option key: {}\n", String::from_utf8_lossy(key)),
    }
}

/// Parses "key=value[,key=value...]" (the payload of an `o` request).
pub fn process_options(options: &[u8]) {
    trace!(
        crate::out::TRACE_FINEST,
        "Processing options \"{}\"\n",
        String::from_utf8_lossy(options)
    );
    for item in options.split(|&c| c == b',') {
        match item.iter().position(|&c| c == b'=') {
            // the value runs up to the next ',' and may contain '=' (the C parser kept
            // only the text after the last '=', which breaks paths containing '=')
            Some(eq) => process_option(&item[..eq], &item[eq + 1..]),
            None => report_error!(
                "Wrong options format: {}\n",
                String::from_utf8_lossy(options)
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forbidden_list_split() {
        set_dirs_forbidden_to_stat(b"/net:/home/x:");
        let s = current();
        assert!(s.is_forbidden(b"/net"));
        assert!(s.is_forbidden(b"/home/x"));
        assert!(!s.is_forbidden(b"/home"));
    }
}
