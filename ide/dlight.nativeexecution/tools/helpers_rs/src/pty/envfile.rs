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

//! The environment file of `--dumpenv`/`--readenv` (tools/pty/src/env.c): a
//! native-endian C `int` with the number of entries, then each `NAME=VALUE`
//! NUL-terminated. Host setup dumps the login shell's environment once
//! (UnixHostInfoProvider); every remote process then starts in it.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

pub fn encode(env: &[Vec<u8>]) -> Vec<u8> {
    let mut data = (env.len() as i32).to_ne_bytes().to_vec();
    for e in env {
        data.extend_from_slice(e);
        data.push(0);
    }
    data
}

pub fn decode(data: &[u8]) -> io::Result<Vec<Vec<u8>>> {
    let bad = |what: &str| io::Error::new(io::ErrorKind::InvalidData, what.to_string());
    let (count, rest) = data
        .split_first_chunk::<4>()
        .ok_or_else(|| bad("truncated header"))?;
    let count = usize::try_from(i32::from_ne_bytes(*count)).map_err(|_| bad("negative count"))?;
    let mut entries: Vec<Vec<u8>> = rest.split(|&b| b == 0).map(<[u8]>::to_vec).collect();
    // the data ends with a NUL, so split leaves one empty piece after it
    if entries.last().is_some_and(Vec::is_empty) {
        entries.pop();
    }
    if entries.len() != count {
        return Err(bad(&format!(
            "{count} entries announced, {} found",
            entries.len()
        )));
    }
    Ok(entries)
}

fn lock(f: &File, kind: libc::c_int, cmd: libc::c_int) -> io::Result<()> {
    // SAFETY: a zeroed flock is a valid "whole file" lock request
    let mut fl: libc::flock = unsafe { std::mem::zeroed() };
    fl.l_type = kind as libc::c_short;
    fl.l_whence = libc::SEEK_SET as libc::c_short;
    // SAFETY: fcntl on an fd we own with a valid flock
    if unsafe { libc::fcntl(f.as_raw_fd(), cmd, &fl) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Why `dumpenv` failed; `Locked` is exit 1, like the C tool's.
#[derive(Debug)]
pub enum DumpError {
    Locked,
    Io(io::Error),
}

/// Writes `env` to `path` (mode 0600) under a write lock. Unlike the C tool,
/// it truncates only after it has the lock, so a reader holding the lock never
/// sees the file emptied.
pub fn dumpenv(path: &Path, env: &[Vec<u8>]) -> Result<(), DumpError> {
    let mut f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)
        .map_err(DumpError::Io)?;
    match lock(&f, libc::F_WRLCK, libc::F_SETLK) {
        Err(e) if matches!(e.raw_os_error(), Some(libc::EACCES | libc::EAGAIN)) => {
            return Err(DumpError::Locked)
        }
        r => r.map_err(DumpError::Io)?,
    }
    f.set_len(0).map_err(DumpError::Io)?;
    f.write_all(&encode(env)).map_err(DumpError::Io)
}

/// Reads an environment written by [`dumpenv`], waiting for a writer to
/// finish. The C tool gave up at once if the file was locked, ignored short
/// reads, and then started the program with an empty environment; this is an
/// error instead.
pub fn readenv(path: &Path) -> io::Result<Vec<Vec<u8>>> {
    let mut f = File::open(path)?;
    lock(&f, libc::F_RDLCK, libc::F_SETLKW)?;
    let mut data = Vec::new();
    f.read_to_end(&mut data)?;
    decode(&data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let env = vec![
            b"A=1".to_vec(),
            b"PATH=/bin:/usr/bin".to_vec(),
            b"EMPTY=".to_vec(),
        ];
        let data = encode(&env);
        assert_eq!(&data[..4], &3i32.to_ne_bytes());
        assert_eq!(&data[4..], b"A=1\0PATH=/bin:/usr/bin\0EMPTY=\0");
        assert_eq!(decode(&data).unwrap(), env);
        assert_eq!(decode(&encode(&[])).unwrap(), Vec::<Vec<u8>>::new());
    }

    #[test]
    fn rejects_bad_files() {
        assert!(decode(b"").is_err());
        assert!(decode(&[1, 0]).is_err());
        let mut data = encode(&[b"A=1".to_vec()]);
        data[0] = 2; // two announced, one present
        assert!(decode(&data).is_err());
        assert!(decode(&(-1i32).to_ne_bytes()).is_err());
    }

    #[test]
    fn dump_then_read() {
        let dir = std::env::temp_dir().join(format!("pty-envfile-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("env");
        std::fs::write(&p, vec![b'x'; 100]).unwrap();
        let env = vec![b"K=V".to_vec()];
        dumpenv(&p, &env).unwrap();
        assert_eq!(readenv(&p).unwrap(), env);
        use std::os::unix::fs::PermissionsExt;
        // an existing file keeps its mode; a new one gets 0600
        let p2 = dir.join("env2");
        dumpenv(&p2, &env).unwrap();
        assert_eq!(
            std::fs::metadata(&p2).unwrap().permissions().mode() & 0o777,
            0o600
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
