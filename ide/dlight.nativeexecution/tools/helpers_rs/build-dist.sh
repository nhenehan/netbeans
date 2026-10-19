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

# Builds static helper binaries and lays them out the way HelperUtility looks
# them up: <outdir>/bin/nativeexecution/<osname>-<platform><_isa>/<helper>
#
# Usage: build-dist.sh <outdir> [rust-target:platform-dir ...]
# Targets that are not installed (rustup target list --installed) are skipped.
# Linux only: other platforms keep the prebuilt binaries from external/.
# unbuffer.so is a no_std cdylib that uses only the host program's libc, so it
# is built from the same musl targets with crt-static off; it has no DT_NEEDED
# and loads into glibc and musl programs alike.

set -eu

helpers="killall pty_open pty"

out=${1:?usage: build-dist.sh <outdir> [target:platform ...]}
shift
[ $# -gt 0 ] || set -- \
    x86_64-unknown-linux-musl:Linux-x86_64 \
    aarch64-unknown-linux-musl:Linux-aarch64 \
    i686-unknown-linux-musl:Linux-x86

here=$(cd "$(dirname "$0")" && pwd)
cd "$here"

installed=$(rustup target list --installed 2>/dev/null || true)
built=0
for spec in "$@"; do
    target=${spec%%:*}
    platform=${spec#*:}
    if ! printf '%s\n' "$installed" | grep -qx "$target"; then
        echo "nativeexecution helpers: skipping $target (not installed; rustup target add $target)" >&2
        continue
    fi
    # musl targets ship a self-contained libc.a, so rust-lld links them without a cross gcc
    var=CARGO_TARGET_$(printf '%s' "$target" | tr 'a-z-' 'A-Z_')_LINKER
    env "$var=rust-lld" cargo build --quiet --release --locked --target "$target" --bins
    flags=CARGO_TARGET_$(printf '%s' "$target" | tr 'a-z-' 'A-Z_')_RUSTFLAGS
    env "$var=rust-lld" "$flags=-C target-feature=-crt-static" \
        cargo build --quiet --release --locked --target "$target" -p unbuffer
    mkdir -p "$out/bin/nativeexecution/$platform"
    cp "target/$target/release/libunbuffer.so" "$out/bin/nativeexecution/$platform/unbuffer.so"
    chmod 755 "$out/bin/nativeexecution/$platform/unbuffer.so"
    echo "nativeexecution helpers: $target -> $out/bin/nativeexecution/$platform/unbuffer.so"
    for h in $helpers; do
        cp "target/$target/release/$h" "$out/bin/nativeexecution/$platform/$h"
        chmod 755 "$out/bin/nativeexecution/$platform/$h"
        echo "nativeexecution helpers: $target -> $out/bin/nativeexecution/$platform/$h"
    done
    built=$((built + 1))
done
[ "$built" -gt 0 ] || { echo "nativeexecution helpers: nothing built" >&2; exit 1; }
