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

# fs_server (Rust)

The remote half of the NetBeans remote file system. The IDE
(`org.netbeans.modules.remote.impl.fs.server.FSSDispatcher`) uploads it to
`<tmp>/bin/<osname>-<platform><_isa>/fs_server` on the remote host and talks
to it over stdin/stdout.

This is a drop-in replacement for the C implementation in `../fs_server`.
It keeps the following the same:

- the command line;
- the line protocol;
- the exit codes;
- the cache layout under `~/.netbeans/remotefs/<dir>` (`dirtab`, `cache/<n>`,
  `lock`, `log`).

It also takes the same `lockf` lock, so the two implementations can share one
cache.

## Build

```sh
cargo build --release
cargo test

# static binaries in the IDE layout (skips targets that aren't installed)
./build-dist.sh /tmp/out    # -> /tmp/out/bin/Linux-x86_64/fs_server, ...
```

The default targets are:

| Rust target | IDE directory |
|---|---|
| `x86_64-unknown-linux-musl` | `Linux-x86_64` |
| `aarch64-unknown-linux-musl` | `Linux-aarch64` (the C zip has no build for this) |
| `armv7-unknown-linux-musleabihf` | `Linux-arm` |

All three link with `rust-lld`; no cross gcc is needed.

## Using it from the IDE

There are two ways:

- **Build it into the cluster.** Run
  `ant -f dlight/dlight.remote.impl/build.xml -Dfs_server.rust=true netbeans`.
  This replaces the C binaries extracted from `external/fs_server-1.0.zip`
  with the Rust ones for the platforms listed above. The other platforms
  (SunOS, FreeBSD, MacOSX-x86, Linux-x86, Linux-sparc) keep the C binaries.
  Without the property, the build is unchanged.
- **Try it without rebuilding.** Start the IDE with
  `-J-Dlocal.fs_server.path=/path/to/fs_server`. You can also use
  `-J-Dremote.fs_server.path=...` to point at a binary that is already on the
  remote host.

## Differential test against the C server

```sh
gcc -O1 -std=gnu99 -D_GNU_SOURCE -D_FORTIFY_SOURCE=0 -pthread -w ../fs_server/src/*.c -o /tmp/fs_server_c
python3 test/compare_with_c.py /tmp/fs_server_c target/release/fs_server
```

The script runs the same request scripts through both servers and compares
their stdout byte for byte. It covers:

- `ls`, recursive `ls`, `stat` and `lstat` over links, unicode names, names
  with blanks, backslash or newline, fifos, files with no access, and options;
- concurrent requests on 8 threads;
- copy, move and delete;
- refresh notifications;
- the `dirtab` file;
- reading the cache in both directions (C reading Rust's, Rust reading C's);
- the IDE's real command lines;
- the cache lock, including `-K` and locks held by the C server;
- delete-on-disconnect.

`-D_FORTIFY_SOURCE=0` is needed because of C bug 1 below.

## Where it deliberately differs from the C server

1. **`-e` really takes an optional file name.** The C getopt string `e:`
   requires an argument, so `-e -c` used `-c` as the log file name and lost
   the cleanup flag.
2. **`-K <msec>` (kill any locker) works.** The C parser always read the
   first number as a PID.
3. **The default for `-e` is `<cache>/stderr.txt`, as documented.** Because
   of initialization order, the C server wrote to `/tmp/fs_server_stderr.txt`.
4. **The second path of `C`/`m` requests is found after the *escaped* first
   path.** The C server skipped the unescaped length, so it mis-parsed copy
   or move requests whose first path contained `\` or a newline.
5. **Option values may contain `=`.**
6. **`dirtab` is written to a temp file and renamed into place.** The C server
   could leave it truncated.
7. **The lock file's `PID=` line is rewritten**, not appended to.
8. **A `readlink` failure reports the link target as `?`.** The C server
   overwrote the file *name* with `?`.
9. **The version is reported as `1.12.9`.** The protocol is unchanged and the
   IDE requires `>= 1.12.0`.

## C bugs found while testing (reasons not to keep shipping the C binary)

1. **Overflow in `getgroups`.** `util.c` `can()` calls
   `getgroups(sizeof groups, groups)`, passing a byte count for a 100-element
   array. With `_FORTIFY_SOURCE` (Ubuntu's default) the server aborts on the
   first `ls` after `o access=fast`, which the IDE sends whenever the access
   check type isn't FULL. Without fortify, this overflows for users who are
   in more than 100 groups.
2. **Double free at exit.** `settings.c` `free_settings()` frees the
   `dirs_forbidden_to_stat` list once per settings version. Copy-on-write
   shares that list between versions, so any option change after the IDE has
   sent the forbidden list causes a double free at exit.
3. The other problems listed in `../fs_server/todo.txt`: unchecked `decode_*`
   results, the flat cache layout, and so on.

## Known limitation in the Java client (not fixed here)

`FSSUtil.escape()` replaces `\n` before `\\`, so a newline in a file name is
escaped twice on the way out. The server unescapes exactly what it receives,
the same way the C server does.

## Later

The process model and protocol are kept as they are so that the IDE side can
stay unchanged. A REST/CoAP or event-driven (inotify/fanotify instead of
polling) interface could be added later behind a new request kind or a
separate binary.
