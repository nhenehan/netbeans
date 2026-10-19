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

# Builds static fs_server binaries and lays them out the way FSSDispatcher looks
# them up: <outdir>/bin/<osname>-<platform><_isa>/fs_server
#
# Usage: build-dist.sh <outdir> [rust-target:platform-dir ...]
# Targets that are not installed (rustup target list --installed) are skipped.
# macOS targets are linked with cargo-zigbuild (cargo install cargo-zigbuild,
# plus zig on the PATH) and skipped without it. The platform directories follow
# FSSDispatcher: Linux gets ${_isa} ("_64", but not for aarch64 or riscv64), macOS does not,
# and Apple silicon reports its CPU family as "arm".

set -eu

out=${1:?usage: build-dist.sh <outdir> [target:platform ...]}
shift
[ $# -gt 0 ] || set -- \
    x86_64-unknown-linux-musl:Linux-x86_64 \
    aarch64-unknown-linux-musl:Linux-aarch64 \
    armv7-unknown-linux-musleabihf:Linux-arm \
    riscv64gc-unknown-linux-musl:Linux-riscv64 \
    x86_64-apple-darwin:MacOSX-x86 \
    aarch64-apple-darwin:MacOSX-arm

here=$(cd "$(dirname "$0")" && pwd)
cd "$here"

installed=$(rustup target list --installed 2>/dev/null || true)
zigbuild=no
cargo zigbuild --help >/dev/null 2>&1 && command -v zig >/dev/null 2>&1 && zigbuild=yes
built=0
for spec in "$@"; do
    target=${spec%%:*}
    platform=${spec#*:}
    if ! printf '%s\n' "$installed" | grep -qx "$target"; then
        echo "fs_server_rs: skipping $target (not installed; rustup target add $target)" >&2
        continue
    fi
    case "$target" in
    *-apple-darwin)
        if [ "$zigbuild" != yes ]; then
            echo "fs_server_rs: skipping $target (needs cargo-zigbuild and zig)" >&2
            continue
        fi
        # zig links against its bundled macOS libc stubs, no Xcode SDK needed;
        # it also ad-hoc signs the arm64 binary, which Apple silicon requires
        # (it warns that no Xcode SDK was found; that is expected)
        cargo zigbuild --quiet --release --locked --target "$target"
        ;;
    *)
        # musl targets ship a self-contained libc.a, so rust-lld links them without a cross gcc
        # +crt-static because some musl targets (riscv64gc) default to dynamic linking
        var=CARGO_TARGET_$(printf '%s' "$target" | tr 'a-z-' 'A-Z_')_LINKER
        flags=CARGO_TARGET_$(printf '%s' "$target" | tr 'a-z-' 'A-Z_')_RUSTFLAGS
        env "$var=rust-lld" "$flags=-C target-feature=+crt-static" \
            cargo build --quiet --release --locked --target "$target"
        ;;
    esac
    mkdir -p "$out/bin/$platform"
    cp "target/$target/release/fs_server" "$out/bin/$platform/fs_server"
    chmod 755 "$out/bin/$platform/fs_server"
    echo "fs_server_rs: $target -> $out/bin/$platform/fs_server"
    built=$((built + 1))
done
[ "$built" -gt 0 ] || { echo "fs_server_rs: nothing built" >&2; exit 1; }
