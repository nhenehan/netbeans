#!/bin/bash
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

# Shared by build-linux-release.sh, build-windows-release.sh and
# build-all-releases.sh. Sourced, not run.
#
# The IDE itself is OS independent: one ant build of the cnd config plus the
# java, web (webcommon, no php) and rust clusters gives a tree that runs on Linux and Windows. The nativeexecution
# helpers for every host OS go into both releases, because the IDE uploads them
# to remote hosts. Only the local launchers and the optional bundled JDK differ
# per OS.

set -euo pipefail

nb_all=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
nbbuild="$nb_all/nbbuild"
dist_dir=${DIST_DIR:-$nbbuild/dist}
work_dir="$dist_dir/.work"

# The cnd config (README.cnd), the java config, nb.cluster.webcommon (HTML,
# CSS, JavaScript; java depends on it anyway) and nb.cluster.rust. php and
# enterprise are left out. Spelled out because ant does not expand ${...} in
# -D values.
cluster_config=cndjavawebrust
cluster_list=nb.cluster.harness,nb.cluster.bootstrap,nb.cluster.platform,nb.cluster.ide,\
nb.cluster.extide,nb.cluster.websvccommon,nb.cluster.webcommon,nb.cluster.java,\
nb.cluster.javafx,nb.cluster.profiler,nb.cluster.3rdparty,nb.cluster.nb,\
nb.cluster.dlight,nb.cluster.cnd,nb.cluster.rust
# cluster directories a --skip-build tree must have
cluster_dirs=(extide websvccommon webcommon java javafx profiler extra nb dlight cnd rust)

rev=$(git -C "$nb_all" rev-parse --short HEAD)
git -C "$nb_all" diff --quiet HEAD 2>/dev/null || rev="$rev-dirty"
release_name=${RELEASE_NAME:-netbeans-cnd-java-web-rust-$(date +%Y%m%d)-$rev}
base_zip="$work_dir/$release_name-base.zip"

log() { printf '==> %s\n' "$*" >&2; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

usage_common() {
    cat <<EOF
  --skip-build        package the existing nbbuild/netbeans tree, don't run ant
  --clean             run 'ant clean' first
  --jdk PATH          bundle this JDK (a directory, .zip or .tar.gz) as netbeans/jdk
  --fetch-jdk VER     download Temurin JDK VER (e.g. 25) for the target OS and bundle it
  -h, --help          show this help

Environment: JAVA_HOME (JDK that runs ant), DIST_DIR (default nbbuild/dist),
RELEASE_NAME (default $release_name), ANT_OPTS.
EOF
}

# Sets skip_build, clean, jdk_src, fetch_jdk from the command line.
parse_args() {
    skip_build=0 clean=0 jdk_src='' fetch_jdk=''
    while [ $# -gt 0 ]; do
        case "$1" in
            --skip-build) skip_build=1 ;;
            --clean) clean=1 ;;
            --jdk) jdk_src=${2:?--jdk needs a path}; shift ;;
            --fetch-jdk) fetch_jdk=${2:?--fetch-jdk needs a version}; shift ;;
            -h|--help) usage; exit 0 ;;
            *) usage >&2; die "unknown option: $1" ;;
        esac
        shift
    done
    [ -z "$jdk_src" ] || [ -z "$fetch_jdk" ] || die "--jdk and --fetch-jdk are exclusive"
}

check_tools() {
    local t
    for t in ant git zip unzip tar; do
        command -v "$t" >/dev/null || die "$t is not installed"
    done
    command -v cargo >/dev/null && command -v rustup >/dev/null ||
        die "the nativeexecution helpers need cargo and rustup (see ide/dlight.nativeexecution/tools/helpers_rs)"
    local missing=''
    for t in x86_64-unknown-linux-musl aarch64-unknown-linux-musl i686-unknown-linux-musl \
             armv7-unknown-linux-musleabihf riscv64gc-unknown-linux-musl; do
        rustup target list --installed | grep -qx "$t" || missing="$missing $t"
    done
    [ -z "$missing" ] || log "warning: Rust targets not installed, their remote helpers will be missing:$missing"
}

# ant_retry_downloads CMD...: runs an ant command, and runs it again (up to
# ANT_DOWNLOAD_ATTEMPTS times, default 3) only when it failed because the
# external binaries could not be downloaded, as when Maven Central has a
# transient outage. Jars that did download stay in */external and the
# binaries cache, and the build is incremental, so a retry resumes. Any other
# failure stops at once.
ant_retry_downloads() {
    local attempts=${ANT_DOWNLOAD_ATTEMPTS:-3} attempt out status
    out=$(mktemp)
    for ((attempt = 1; ; attempt++)); do
        status=0
        "$@" 2>&1 | tee "$out" || status=$?
        [ "$status" = 0 ] && break
        if [ "$attempt" -ge "$attempts" ] ||
           ! grep -q 'Failed to download binaries' "$out"; then
            rm -f "$out"
            return "$status"
        fi
        log "external binaries failed to download (attempt $attempt of $attempts); retrying in $((attempt * 30))s"
        sleep $((attempt * 30))
    done
    rm -f "$out"
}

# Builds the IDE once (unless --skip-build) and zips it the way nbbuild's
# zip-cluster-config does, which applies the executable bits and leaves out
# the build's bookkeeping files.
build_ide() {
    mkdir -p "$work_dir"
    local ant=(ant -f "$nbbuild/build.xml"
               "-Dcluster.config=$cluster_config"
               "-Dclusters.config.$cluster_config.list=$cluster_list")
    if [ "$clean" = 1 ]; then
        log "ant clean"
        "${ant[@]}" clean
    fi
    if [ "$skip_build" = 1 ]; then
        [ -x "$nbbuild/netbeans/bin/netbeans" ] || die "--skip-build, but nbbuild/netbeans has no build"
        local d
        for d in "${cluster_dirs[@]}"; do
            [ -d "$nbbuild/netbeans/$d" ] || die "--skip-build, but nbbuild/netbeans has no $d cluster; build without --skip-build"
        done
    else
        log "ant build-nozip (clusters: $cluster_list)"
        ant_retry_downloads "${ant[@]}" build-nozip
    fi
    [ -f "$nbbuild/netbeans/ide/bin/nativeexecution/Linux-x86_64/pty" ] ||
        die "the Rust nativeexecution helpers are missing from the build"
    rm -f "$base_zip"
    log "zipping the build"
    "${ant[@]}" -Ddo-not-rebuild-clusters=true "-Dzip.name=$base_zip" zip-cluster-config
}

# stage OS: unpacks the base zip into $stage/netbeans and drops the other
# OS's launchers.
stage() {
    local os=$1
    stage_dir="$work_dir/$os"
    rm -rf "$stage_dir"
    mkdir -p "$stage_dir"
    unzip -q "$base_zip" -d "$stage_dir"
    local nb="$stage_dir/netbeans"
    [ -d "$nb" ] || die "the base zip has no netbeans/ directory"
    case "$os" in
        linux)
            rm -f "$nb"/bin/*.exe "$nb"/platform/lib/*.exe "$nb"/platform/lib/*.dll
            ;;
        windows)
            rm -f "$nb/bin/netbeans" "$nb/platform/lib/nbexec"
            ;;
    esac
}

# bundle_jdk OS: puts the --jdk / --fetch-jdk JDK into netbeans/jdk and points
# etc/netbeans.conf at it. Both launchers expand ${BASEDIR} in netbeans_jdkhome.
bundle_jdk() {
    local os=$1 src=$jdk_src
    [ -n "$src" ] || [ -n "$fetch_jdk" ] || return 0
    [ -n "$src" ] || src=$(fetch_temurin "$os" "$fetch_jdk")
    local nb="$stage_dir/netbeans" tmp="$stage_dir/jdk.tmp"
    rm -rf "$tmp" "$nb/jdk"
    if [ -d "$src" ]; then
        cp -a "$src" "$nb/jdk"
    else
        mkdir -p "$tmp"
        case "$src" in
            *.zip) unzip -q "$src" -d "$tmp" ;;
            *.tar.gz|*.tgz) tar -xzf "$src" -C "$tmp" ;;
            *) die "--jdk: not a directory, .zip or .tar.gz: $src" ;;
        esac
        # archives hold a single jdk-<version>/ directory
        local top
        top=$(find "$tmp" -mindepth 1 -maxdepth 1)
        [ "$(printf '%s\n' "$top" | wc -l)" = 1 ] && [ -d "$top" ] || die "unexpected JDK archive layout: $src"
        mv "$top" "$nb/jdk"
        rm -rf "$tmp"
    fi
    local java=bin/java
    [ "$os" = windows ] && java=bin/java.exe
    [ -f "$nb/jdk/$java" ] || die "no $java in the bundled JDK (is it a $os JDK?)"
    sed -i 's|^#\?netbeans_jdkhome=.*|netbeans_jdkhome="${BASEDIR}/jdk"|' "$nb/etc/netbeans.conf"
    grep -q '^netbeans_jdkhome="${BASEDIR}/jdk"' "$nb/etc/netbeans.conf" || die "could not set netbeans_jdkhome"
    log "bundled JDK $(cat "$nb/jdk/release" 2>/dev/null | sed -n 's/^JAVA_VERSION="\(.*\)"/\1/p')"
}

# fetch_temurin OS VERSION: downloads the latest Temurin GA JDK for x64 into
# $dist_dir/jdks (cached), checks its SHA-256 and prints the archive path.
fetch_temurin() {
    local os=$1 ver=$2 api_os
    case "$os" in linux) api_os=linux ;; windows) api_os=windows ;; esac
    command -v curl >/dev/null && command -v jq >/dev/null || die "--fetch-jdk needs curl and jq"
    local meta
    meta=$(curl -fsSL "https://api.adoptium.net/v3/assets/latest/$ver/hotspot?architecture=x64&image_type=jdk&os=$api_os&vendor=eclipse") ||
        die "could not query api.adoptium.net for JDK $ver"
    local name link sum
    name=$(jq -r '.[0].binary.package.name' <<<"$meta")
    link=$(jq -r '.[0].binary.package.link' <<<"$meta")
    sum=$(jq -r '.[0].binary.package.checksum' <<<"$meta")
    [ "$name" != null ] || die "no Temurin JDK $ver for $api_os x64"
    mkdir -p "$dist_dir/jdks"
    local file="$dist_dir/jdks/$name"
    if [ ! -f "$file" ] || ! echo "$sum  $file" | sha256sum -c --status; then
        log "downloading $name"
        curl -fL --progress-bar -o "$file.part" "$link"
        echo "$sum  $file.part" | sha256sum -c --status || die "checksum mismatch for $name"
        mv "$file.part" "$file"
    fi
    printf '%s\n' "$file"
}

# archive OS: writes the release archive and its .sha256 into $dist_dir.
archive() {
    local os=$1 out
    mkdir -p "$dist_dir"
    case "$os" in
        linux)
            out="$dist_dir/$release_name-linux-x64.tar.gz"
            tar -C "$stage_dir" --owner=0 --group=0 -czf "$out" netbeans
            ;;
        windows)
            out="$dist_dir/$release_name-windows-x64.zip"
            rm -f "$out"
            (cd "$stage_dir" && zip -qr -X "$out" netbeans)
            ;;
    esac
    (cd "$dist_dir" && sha256sum "$(basename "$out")" >"$(basename "$out").sha256")
    rm -rf "$stage_dir"
    log "$out"
}

package() {
    local os=$1
    log "packaging $os"
    stage "$os"
    bundle_jdk "$os"
    archive "$os"
}
