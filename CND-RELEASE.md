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

# CND + Java + Web + Rust release build

How to build, install and package this branch (`cnd-stable`) as a
self-contained IDE for Linux x64 and Windows x64, built entirely on Linux.

This is a private build from a git checkout. Per [PACKAGING.md](PACKAGING.md)
it must not be distributed or labelled as an Apache NetBeans release, which
is why the archives and the Windows installer use their own names.

## What is in the build

| Cluster | Provides |
|---|---|
| `platform`, `harness`, `ide`, `nb`, `extra` | the base IDE |
| `cnd`, `dlight` | C/C++ (the normal cnd build), remote hosts, nativeexecution helpers |
| `java`, `extide`, `websvccommon`, `javafx`, `profiler` | Java SE: Maven, Gradle, Ant, debugger, profiler |
| `webcommon` | HTML5, CSS, JavaScript, Node.js (the `java` cluster depends on it anyway) |
| `rust` | Rust projects, Cargo, rust-analyzer integration |

Not included: `php`, `enterprise` (Jakarta EE), `groovy`, `apisupport`,
`ergonomics`, `cpplite`.

The cluster list lives in one place: `cluster_list` in
`nbbuild/cnd-release/common.sh`.

Both releases contain the same IDE. Only the launchers (`bin/netbeans` vs.
`bin/netbeans64.exe`) and the bundled JDK differ. The nativeexecution helpers
for every remote host OS go into both, because the IDE uploads them to remote
hosts.

## Prerequisites (Linux build host)

| Tool | Used for | Checked with |
|---|---|---|
| JDK 17+ on `PATH` or `JAVA_HOME` (JDK 25 used here) | running ant | `java -version` |
| Apache Ant 1.10 | the build | `ant -version` |
| Rust via rustup, with cargo | nativeexecution helpers | `cargo --version` |
| musl targets (see below) | helpers for remote Linux hosts | `rustup target list --installed` |
| `git`, `zip`, `unzip`, `tar` | packaging | |
| `curl`, `jq` | `--fetch-jdk` only | |

```sh
rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl \
    i686-unknown-linux-musl armv7-unknown-linux-musleabihf riscv64gc-unknown-linux-musl
```

A missing target only drops the helper for that remote architecture; the
script warns about it and carries on.

Disk: allow about 10 GB for the build tree, the two archives and the JDK
downloads.

## Build

From the repository root:

```sh
nbbuild/cnd-release/build-all-releases.sh --fetch-jdk 25
```

This runs one ant build of all the clusters above, then packages it twice.
Output in `nbbuild/dist/`:

```
netbeans-cnd-java-web-rust-<yyyymmdd>-<rev>[-dirty]-linux-x64.tar.gz      (+ .sha256)
netbeans-cnd-java-web-rust-<yyyymmdd>-<rev>[-dirty]-windows-x64.zip       (+ .sha256)
```

`-dirty` is added when the working tree has uncommitted changes.

### Scripts

| Script | Produces |
|---|---|
| `build-all-releases.sh` | both archives from one build |
| `build-linux-release.sh` | the Linux `.tar.gz` only |
| `build-windows-release.sh` | the Windows `.zip` only |

### Options

| Option | Effect |
|---|---|
| `--fetch-jdk VER` | download the latest Temurin JDK `VER` for each target OS (cached and checksum-verified in `nbbuild/dist/jdks/`) and bundle it as `netbeans/jdk` |
| `--jdk PATH` | bundle this JDK (directory, `.zip` or `.tar.gz`). One OS only, so use it with the per-OS scripts, not `build-all-releases.sh` |
| `--skip-build` | package the existing `nbbuild/netbeans` tree without running ant. Refuses if a cluster is missing |
| `--clean` | run `ant clean` first |

Without a JDK option, the IDE uses whatever JDK it finds on the target machine.

| Environment variable | Default |
|---|---|
| `DIST_DIR` | `nbbuild/dist` |
| `RELEASE_NAME` | `netbeans-cnd-java-web-rust-<date>-<rev>` |
| `ANT_OPTS` | (ant default) |

### Checking a build

```sh
cd nbbuild/dist
sha256sum -c netbeans-cnd-java-web-rust-*.sha256
tar -tzf netbeans-cnd-java-web-rust-*-linux-x64.tar.gz | awk -F/ 'NF>2{print $2}' | sort -u
unzip -Z1 netbeans-cnd-java-web-rust-*-windows-x64.zip | awk -F/ 'NF>2{print $2}' | sort -u
```

Both listings should show `cnd dlight extide java javafx jdk profiler rust webcommon websvccommon`,
plus the base clusters, and no `php`.

### Rebuilding after a change to one module

A full build is not needed for a fix in one module. Build that module into
`nbbuild/netbeans`, then repackage:

```sh
ant -f ide/lsp.client/build.xml netbeans          # the changed module
nbbuild/cnd-release/build-all-releases.sh --skip-build --fetch-jdk 25
```

## Downloads from GitHub

`.github/workflows/cnd-release.yml` runs the build below on a GitHub runner
(about 1–2 hours). It publishes the Linux `.tar.gz`, the Windows `.zip` and the
NSIS installer `.exe`, each with a bundled JDK and a `.sha256`:

| Trigger | Where the files appear |
|---|---|
| push a tag `cnd-*`, e.g. `git tag cnd-2026.11.19 && git push origin cnd-2026.11.19` | a GitHub Release named after the tag |
| push to `cnd-stable` | the pre-release **cnd-stable-latest**, replaced on every push |
| Actions > CND release build > Run workflow, on any branch | workflow artifacts, kept 7 days |

The usual flow: work and upstream merges go to `cnd-develop`. When it is
verified, fast-forward `cnd-stable` to it and push; that publishes
cnd-stable-latest. Tag a commit to keep a build permanently.

## Install

### Linux

Close NetBeans first. Keep the previous install as a backup:

```sh
mv ~/Applications/netbeans ~/Applications/netbeans-previous
tar -xzf nbbuild/dist/netbeans-cnd-java-web-rust-*-linux-x64.tar.gz -C ~/Applications
~/Applications/netbeans/bin/netbeans
```

The user directory (`~/.netbeans/dev`) and cache (`~/.cache/netbeans/dev`)
carry over. The first start takes longer while the new modules are registered.

### Windows (zip)

Unzip the `-windows-x64.zip` anywhere and run `netbeans\bin\netbeans64.exe`.
For an installer with Start menu entries and an uninstaller, see below.

To build C/C++ projects in WSL2 distributions (one or several), see
[CND-WSL2.md](CND-WSL2.md).

## Rust: rust-analyzer

Code completion, navigation and diagnostics for Rust come from rust-analyzer:

```sh
rustup component add rust-analyzer
```

Then set its path in **Tools → Options → Rust → rust-analyzer**
(`~/.cargo/bin/rust-analyzer` on Linux).

This build applies the setting on the next start, and it also repairs the
stale-cache case below. If Rust completion shows "No suggestions" on an older
build, close the IDE and delete `~/.cache/netbeans/dev` (it holds only caches).

## Windows installer (NSIS)

The installer script is `nbbuild/cnd-release/netbeans-installer.nsi`. It is
compiled on Linux with `makensis`, from the contents of the Windows zip.

Tested so far: steps 1–3 below, run exactly as written with the container
variant, compile with NSIS 3.09 without warnings. The installer contains every
file from the zip plus the uninstaller, and carries the version and product
name. It has not yet been run on Windows; do a test install there before
handing it out.

### What the installer does

- 64-bit Windows only; asks for administrator rights.
- Installs to `C:\Program Files\NetBeans-cnd-java-web-rust` (changeable).
- Shows the Apache License from the build's `LICENSE`.
- Adds a Start menu folder (IDE and Uninstall) and a desktop shortcut, all
  pointing at `bin\netbeans64.exe` with the NetBeans icon.
- Registers in **Settings → Apps** with version, size and a silent uninstall
  string.
- The uninstaller removes the install directory (only if it contains
  `bin\netbeans64.exe` and `etc\netbeans.conf`), the shortcuts and the
  registry entry. It leaves the user directory (`%APPDATA%\NetBeans`) and
  cache (`%LOCALAPPDATA%\NetBeans\Cache`) alone.

### 1. Install NSIS

Either from the distribution:

```sh
sudo apt install nsis          # Ubuntu/Debian; provides makensis 3.x
```

or with no root access, through a container (podman or docker), used in the
steps below as `MAKENSIS`:

```sh
MAKENSIS() {
    podman run --rm -v "$PWD":/w:Z -w /w docker.io/library/ubuntu:24.04 bash -c \
        "apt-get update -qq >/dev/null && apt-get install -y -qq nsis >/dev/null 2>&1 && makensis $*"
}
```

With a native install, use `makensis` directly instead of `MAKENSIS`.

### 2. Stage the Windows build

```sh
cd nbbuild/dist
ZIP=$(ls netbeans-cnd-java-web-rust-*-windows-x64.zip)
REL=${ZIP%-windows-x64.zip}
rm -rf nsis-stage && mkdir nsis-stage
unzip -q "$ZIP" -d nsis-stage
du -sk nsis-stage/netbeans | cut -f1 > nsis-stage/size-kb.txt   # shown in Apps & features
cp ../cnd-release/netbeans-installer.nsi nsis-stage/
```

### 3. Compile

`VERSION` must be three numbers; the build date works well.

```sh
cd nsis-stage
VERSION=$(echo "$REL" | sed -E 's/.*-([0-9]{4})([0-9]{2})([0-9]{2})-.*/\1.\2.\3/')
MAKENSIS -V2 -DSRC=netbeans -DVERSION=$VERSION -DOUTFILE=$REL-windows-x64.exe netbeans-installer.nsi
mv $REL-windows-x64.exe .. && cd ..
sha256sum $REL-windows-x64.exe > $REL-windows-x64.exe.sha256
```

Expect a 320 MB installer (LZMA solid compression of about 880 MB of files).
Steps 2 and 3 took about 7 minutes here with the container variant, which
installs NSIS on every run.

Do not name the output `setup.exe`, `install.exe` or similar. Windows applies
compatibility shims to such names, and makensis warns about it.

Optional defines:

| Define | Default |
|---|---|
| `-DNAME="..."` | `NetBeans CND-Java-Web-Rust (unofficial build)`, used for the window title, shortcuts and Apps & features |
| `-DOUTFILE=...` | `netbeans-cnd-java-web-rust-windows-x64.exe` |

If you change `NAME`, keep it clearly unofficial (see the PACKAGING.md note
at the top).

### 4. Verify

```sh
7z l $REL-windows-x64.exe | tail -3        # file count should match the zip, plus uninstall.exe
7z l $REL-windows-x64.exe | grep -E ' (bin/netbeans64.exe|jdk/bin/java.exe|uninstall.exe)$'
```

(`7z` is in the `p7zip-full` package.)

### 5. Sign (optional, recommended)

An unsigned installer triggers SmartScreen's "Windows protected your PC"
warning. To sign on Linux with a code-signing certificate:

```sh
sudo apt install osslsigncode
osslsigncode sign -pkcs12 codesign.pfx -askpass \
    -n "NetBeans CND-Java-Web-Rust" -t http://timestamp.digicert.com \
    -in $REL-windows-x64.exe -out $REL-windows-x64-signed.exe
osslsigncode verify $REL-windows-x64-signed.exe
```

Signing the installer is enough for the download warning. The bundled
`netbeans64.exe` and JDK binaries keep their own signatures, or lack of them.

### 6. Install, silent install and uninstall on Windows

| Action | Command |
|---|---|
| Interactive install | run the `.exe` |
| Silent install | `netbeans-cnd-java-web-rust-...-windows-x64.exe /S` |
| Silent install, other directory | `... /S /D=D:\Tools\NetBeans` (`/D=` must be last, no quotes) |
| Uninstall | Settings → Apps, or Start menu → Uninstall |
| Silent uninstall | `"C:\Program Files\NetBeans-cnd-java-web-rust\uninstall.exe" /S` |

Installing a newer build over an older one replaces the files in place. To
avoid leftovers from removed modules, uninstall first.

### Alternative: NBPackage

Apache's [NBPackage](https://github.com/apache/netbeans-nbpackage) builds
Windows installers with Inno Setup (plus `.deb`, `.rpm` and AppImage) from a
binary zip, and can bundle a JDK. It does not produce NSIS installers, and
Inno Setup needs Windows or Wine to compile. NSIS is used here because it
compiles natively on Linux.

## Troubleshooting

| Symptom | Cause and fix |
|---|---|
| No Java or HTML5 categories in New Project | An older cnd+rust install is still the one being run. Check that the install directory has `java/` and `webcommon/`, and reinstall from the `netbeans-cnd-java-web-rust-*` archive. |
| Rust completion says "No suggestions" | rust-analyzer not set in Options, or an old build with a stale layer cache (delete `~/.cache/netbeans/dev`). |
| `error: the Rust nativeexecution helpers are missing from the build` | cargo or rustup not found while building `ide/dlight.nativeexecution`. Install them and rebuild. |
| `--skip-build, but nbbuild/netbeans has no <x> cluster` | The tree was built with a different cluster list. Run without `--skip-build`. |
| `--jdk bundles one OS's JDK` | Use `--fetch-jdk`, or `--jdk` with a per-OS script. |
| SmartScreen blocks the installer | It is unsigned; sign it (step 5) or choose "More info → Run anyway". |
| Red error badge from "content modified" errors with Rust | Fixed in this build: the LSP client no longer reports these routine rust-analyzer replies as errors. |

## Changes on this branch that this build depends on

| Area | Change |
|---|---|
| `nbbuild/cnd-release/` | release scripts; cluster list extended with java and web; NSIS script |
| `rust/rust.options`, `rust/rust.grammar` | rust-analyzer mode is re-applied when the setting changes and at startup, so a cached layer from before rust-analyzer was configured no longer disables Rust completion |
| `ide/lsp.client` | `ContentModified`, `RequestCancelled` and `ServerCancelled` replies are logged at FINE instead of reported as errors |
