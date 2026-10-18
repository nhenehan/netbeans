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
| `pty_open` | `tools/pty_open.c` | `pty/PtyOpenUtility`, `pty/PtyAllocator` (the debugger's internal terminal) |
| `pty` ("nbstart") | `tools/pty` | `pty/NbStartUtility`, `NbNativeProcess` (starts every process), host setup (`--dumpenv`) |

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

## pty_open

Allocates a pseudo terminal and prints a header that `PtyOpenUtility` parses:

```
PID: <its pid>
TTY: /dev/pts/<n>

```

It keeps the slave open, so the pty survives debuggees that open and close it.
Then it relays bytes from its stdin to the pty master, and from the master to
its stdout, until stdin reaches EOF or hangs up. On exit it returns 0. Errors go
to stdout as `pty_open: <message>`, with exit 1. `PtyAllocator` starts it with
`exec` through `sh -s`, so the pid it reports is the shell's.
`tests/pty_open.rs` runs that path, and with `PTY_OPEN_C` set it compares a whole
terminal session with the C tool.

Two differences from the C tool, both fixes:

- Writes are complete. The C tool ignored short writes.
- Input that is still readable when stdin hangs up is passed on before exiting.
  The C tool stopped at the hangup.

## pty (nbstart)

Starts a program for `NbNativeProcess` in one of three ways:

- on a newly allocated pseudo terminal, relaying it to the IDE;
- on a given one (`-p PTS`);
- on its own stdio (`--no-pty`).

It prints a `PID=`/`TTY=`/`PSID=`/[`REPORT=`]/`NBMAGIC=` header, ended by an empty
line. It puts `NBMAGIC=<its pid>` into the program's environment, which is how
Stop finds and kills the whole process tree (`killall -e`). It exits with the
program's exit code, or 1 if the program was killed by a signal. A program that
can't be started gives 127 (not found) or 1. `--report FILE` writes
`FILE.<pid>` for `ProcessStatusEx`. `--dumpenv FILE` and `--readenv FILE` record an
environment and start in it: remote host setup records the login shell's
environment once, and every remote process starts in it. The full protocol is in
`plan/n2-pty-survey.md`.

Differences from the C tool, all fixes:

- `--readenv` sets the environment in place. The C tool forked an extra process
  for it, which exited 1 if the program was stopped.
- A missing or corrupt env file is an error. The C tool built an unterminated
  environment and fell back to an empty one.
- Once the program has closed the terminal, Linux returns EIO on the master;
  that now counts as EOF. The C tool exited 1 there, before reaping the program,
  which lost its exit code and report. Output still buffered when the program
  exits is drained.
- EOF on stdin without a hangup (`/dev/null`) only stops input. The C tool also
  stopped relaying output, header included.
- After a hangup, it stops sending SIGTERM as soon as the program has exited.
  The C tool's `kill(pid, 0)` check also succeeds on a zombie, so it always waited
  3 s and sent SIGKILL.
- `-w` blocks SIGCONT from the start, so an early SIGCONT isn't lost.
- The report's UTIME/STIME are always ms (the C fallback printed seconds). The
  `SIG:` text uses glibc's wording on musl too.
- `--no-pty` calls `setsid` only in the program's process, not also in pty.
- `--dumpenv` truncates only after it holds the lock. `--readenv` waits for the
  lock instead of failing.
- `wait` stops on ECHILD instead of looping.

## Build

```sh
cargo test
KILLALL_C=/path/to/c/killall PTY_OPEN_C=/path/to/c/pty_open PTY_C=/path/to/c/pty cargo test   # also compare with the C tools

# static musl binaries in the IDE layout (skips targets that aren't installed)
./build-dist.sh /tmp/out    # -> /tmp/out/bin/nativeexecution/Linux-x86_64/{killall,pty_open,pty}, ...
```

`ant build` runs `build-dist.sh` (see `../../build.xml`). The default targets are
`x86_64-unknown-linux-musl` (`Linux-x86_64`) and `aarch64-unknown-linux-musl`
(`Linux-aarch64`). Both link statically with `rust-lld`, so the binaries do not
depend on the host's glibc version.
