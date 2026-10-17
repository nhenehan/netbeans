#!/bin/sh
# Licensed to the Apache Software Foundation (ASF) under one
# or more contributor license agreements.  See the NOTICE file
# distributed with this work for additional information
# regarding copyright ownership.  The ASF licenses this file
# to you under the Apache License, Version 2.0 (the
# "License"); you may not use this file except in compliance
# with the License.  You may obtain a copy of the License at
#
#   http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing,
# software distributed under the License is distributed on an
# "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
# KIND, either express or implied.  See the License for the
# specific language governing permissions and limitations
# under the License.

# Runs a module's unit tests against the throwaway sshd container defined by the
# Dockerfile next to this script (listening on 127.0.0.1:2222 only).
#
# Usage: run-remote-tests.sh <module-dir> [extra ant args...]
#   e.g. run-remote-tests.sh dlight/dlight.remote.impl -Dfs_server.rust=true

set -eu
here=$(cd "$(dirname "$0")" && pwd)
nb_all=$(cd "$here/../../../.." && pwd)
module=${1:?usage: run-remote-tests.sh <module-dir> [ant args...]}
shift

engine=$(command -v podman || command -v docker)
"$engine" build -q -t nb-remote-sshd:bookworm "$here" >/dev/null
if ! "$engine" ps --format '{{.Names}}' | grep -qx nb-remote-sshd; then
    "$engine" rm -f nb-remote-sshd >/dev/null 2>&1 || true
    "$engine" run -d --name nb-remote-sshd -p 127.0.0.1:2222:22 nb-remote-sshd:bookworm >/dev/null
    sleep 2
fi

cd "$nb_all"
rm -rf "$module/build/test/unit/work"
exec ant -f "$module/build.xml" test \
    -Dtest-unit-sys-prop.cnd.remote.rcfile="$here/cndtestrc" \
    -Dtest-unit-sys-prop.cnd.remote.testuserinfo.rcfile="$here/testuserinfo" \
    "$@"
