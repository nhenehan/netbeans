<!--
    Licensed to the Apache Software Foundation (ASF) under one
    or more contributor license agreements.  See the NOTICE file
    distributed with this work for additional information
    regarding copyright ownership.  The ASF licenses this file
    to you under the Apache License, Version 2.0 (the
    "License"); you may not use this file except in compliance
    with the License.  You may obtain a copy of the License at

      http://www.apache.org/licenses/LICENSE-2.0

    Unless required by applicable law or agreed to in writing,
    software distributed under the License is distributed on an
    "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
    KIND, either express or implied.  See the License for the
    specific language governing permissions and limitations
    under the License.
-->

# nativeexecution helpers (Rust, Linux)

Rust ports of the native helpers that `dlight.nativeexecution` runs locally and
uploads to remote hosts. The IDE finds them through `HelperUtility` as
`bin/nativeexecution/<osname>-<platform><_isa>/<helper>`. This crate only builds
Linux binaries. The other platforms keep the prebuilt binaries from `external/`,
and so do the helpers that have not been ported yet.

| Helper | Replaces | Java side |
|---|---|---|
| `killall` | `tools/killall` | `signals/impl/NbKillAllSignalSupport` |

## killall

```
killall -p|-g|-s SIGNAL id      # pid, process group, session
killall -q SIGNAL pid value     # sigqueue with an integer value
killall [-n] -e SIGNAL NAME=VALUE   # every process with that environment entry (-n: list pids only)
```

`SIGNAL` is a name without the `SIG` prefix (`TERM`, `INT`, ...), or `NULL` for signal 0.
The exit code is the result of the system call: 0 on success, or 255 when it failed.
`-e` always exits 0, and usage errors exit 1. The messages and exit codes match the
C tool, which `tests/killall.rs` checks when `KILLALL_C` points at a C build.

Two differences from the C tool, both fixes:

- On Linux, `-s` signals every process in the session (found from `/proc/<pid>/stat`).
  The C tool's `-s` was undefined on Linux.
- `-e` reads the whole `/proc/<pid>/environ`, not just its first 4048 bytes.

## Build

```sh
cargo test
KILLALL_C=/path/to/c/killall cargo test     # also compare with the C tool

# static musl binaries in the IDE layout (skips targets that aren't installed)
./build-dist.sh /tmp/out    # -> /tmp/out/bin/nativeexecution/Linux-x86_64/killall, ...
```

`ant build` runs `build-dist.sh` (see `../../build.xml`). The default targets are
`x86_64-unknown-linux-musl` (`Linux-x86_64`) and `aarch64-unknown-linux-musl`
(`Linux-aarch64`). Both link statically with `rust-lld`, so the binaries do not
depend on the host's glibc version.
