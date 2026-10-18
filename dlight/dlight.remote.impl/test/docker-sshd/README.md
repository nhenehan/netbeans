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

# Throwaway remote host for the dlight/cnd remote tests

The remote file system tests (and other cnd/dlight remote tests) run against
the hosts listed in the `[remote.platforms]` section of an rc file. Without
such a host they fail with
`Can not find key intel-S2 in [remote.platforms] section`.

This directory provides that host: a Debian container with sshd, gcc/gdb,
git/hg/svn and a JRE, listening on 127.0.0.1:2222 only. It comes with
matching rc files (`cndtestrc`, `testuserinfo`), so nothing has to be written
to `~/.cndtestrc` or `~/.testuserinfo`.

```sh
xvfb-run -a dlight/dlight.remote.impl/test/docker-sshd/run-remote-tests.sh dlight/dlight.remote.impl
```

The script uses podman if it is installed, otherwise docker. The user and
password are `nbtest`/`nbtest`; they are only reachable from localhost and are
for testing only.

Baseline on 2026-10-16 (master d8e2006421, `dlight.remote.impl`): 271 tests,
270 pass.

| fs_server | Failures |
|---|---|
| Rust (the default since the C sources and binaries were removed) | `testMultipleRead` only |
| C (prebuilt CDDL binaries, before that) | same as Rust |

`testMultipleRead` is a timing flake over loopback: it requires later passes
to take at most 1/8 of the first pass, and the first pass takes only ~5 ms.

Two details of the image matter:

- `/usr/include/sys` is a real directory, not a symlink to the multiarch
  directory, because `testDots` checks how `..` resolves.
- The JRE is there for the remote "agent" that `FileOperationsTestCase` runs.
