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

# WSL2 distributions as C/C++ build hosts

How to use one or more WSL2 distributions (Ubuntu, Debian, Fedora, ...) as
remote build hosts for C/C++ projects in this build on Windows.

The IDE has no direct WSL integration yet. It reaches a distribution the same
way it reaches any Linux machine: over SSH. Each distribution therefore needs
its own SSH server, and with more than one distribution, each SSH server needs
its **own port**. That is the part that usually goes wrong.

## Why several distributions are tricky

* **All WSL2 distributions share one network.** They run in one virtual machine
  with one IP address, so `localhost:22` cannot reach two distributions at once.
  If two of them try to run `sshd` on port 22, the second fails to start, or you
  end up connected to the wrong distribution without noticing.
* **Windows may own port 22 already.** If the Windows "OpenSSH Server" feature is
  on, `localhost:22` is Windows itself, not Linux.
* **Distributions stop when idle.** WSL shuts a distribution down after a while
  when nothing from Windows is attached to it, and its `sshd` stops with it.
  The IDE then shows the host as *No SSH Access* until the distribution runs
  again.
* **Files live in two worlds.** Windows files appear in Linux as `/mnt/c/...`,
  which is slow. Each distribution's files appear in Windows as
  `\\wsl.localhost\<Distro>\...`. The IDE does not map these paths automatically;
  it only maps network drive letters.

The fix: one fixed port per distribution, SSH started by systemd, and one IDE
host entry per distribution.

## 1. Pick a port per distribution

List your distributions in PowerShell:

```powershell
wsl -l -v
```

Choose a port for each one and write the list down. Do not use 22.

| Distribution | Port | IDE display name |
|---|---|---|
| `Ubuntu-24.04` | 2201 | WSL Ubuntu 24.04 |
| `Debian` | 2202 | WSL Debian |
| `FedoraLinux-42` | 2203 | WSL Fedora |

## 2. Set up each distribution

Run these steps inside **each** distribution (`wsl -d <Distro>`), using that
distribution's port.

### 2a. Turn on systemd

```sh
sudo tee /etc/wsl.conf >/dev/null <<'EOF'
[boot]
systemd=true
EOF
```

In PowerShell, run `wsl --terminate <Distro>`, then open the distribution again.
`systemctl is-system-running` should print `running` or `degraded`.

### 2b. Install the SSH server and the tools

```sh
# Ubuntu / Debian
sudo apt update && sudo apt install -y openssh-server build-essential gdb cmake
# Fedora
sudo dnf install -y openssh-server gcc gcc-c++ make gdb cmake
```

### 2c. Give `sshd` its own port

```sh
PORT=2201            # this distribution's port from step 1
sudo mkdir -p /etc/ssh/sshd_config.d
echo "Port $PORT" | sudo tee /etc/ssh/sshd_config.d/10-wsl-port.conf
```

Then start the server so it stays on after restarts:

```sh
# Ubuntu 22.10 and later start ssh through a socket. The socket takes its port
# from sshd_config, so reload it after changing the port:
sudo systemctl daemon-reload
sudo systemctl restart ssh.socket 2>/dev/null || true
sudo systemctl enable --now ssh 2>/dev/null || sudo systemctl enable --now sshd
```

Check it, inside the distribution:

```sh
ss -tlnp | grep ":$PORT"     # sshd must be listening on this port
```

Ubuntu 24.04 can keep listening on 22 even after the change. If it does, set
the socket's port directly:

```sh
sudo systemctl edit ssh.socket
#   [Socket]
#   ListenStream=
#   ListenStream=2201
sudo systemctl restart ssh.socket
```

### 2d. Log in with an SSH key

Password login works, but keys avoid a prompt for every host. In PowerShell,
create one key once and reuse it for every distribution:

```powershell
ssh-keygen -t ed25519 -f $env:USERPROFILE\.ssh\id_ed25519_wsl
```

Then, inside each distribution:

```sh
mkdir -p ~/.ssh && chmod 700 ~/.ssh
cat /mnt/c/Users/<WindowsUser>/.ssh/id_ed25519_wsl.pub >> ~/.ssh/authorized_keys
chmod 600 ~/.ssh/authorized_keys
```

### 2e. Test from Windows

```powershell
ssh -i $env:USERPROFILE\.ssh\id_ed25519_wsl -p 2201 <linuxuser>@127.0.0.1 'cat /etc/os-release | head -1'
```

Each port must print **its own** distribution's name. Use `127.0.0.1` rather
than `localhost`: WSL forwards IPv4 localhost to Linux, and `localhost` can
resolve to IPv6 `::1` first.

## 3. Keep the distributions running

A distribution must be running before the IDE can connect to it. Two options:

* **By hand:** before you work, open the distribution once, or run
  `wsl -d <Distro> --exec true` in PowerShell. That starts the distribution, and
  systemd starts `sshd` with it.
* **At logon:** create a Windows scheduled task per distribution that runs at
  logon and keeps it alive:

  ```powershell
  $a = New-ScheduledTaskAction -Execute "wsl.exe" -Argument "-d Ubuntu-24.04 --exec sleep infinity"
  $t = New-ScheduledTaskTrigger -AtLogOn
  Register-ScheduledTask -TaskName "WSL keepalive Ubuntu-24.04" -Action $a -Trigger $t `
      -Settings (New-ScheduledTaskSettingsSet -ExecutionTimeLimit 0 -Hidden)
  ```

  The idle-shutdown rules differ between WSL releases. If a distribution still
  stops, check `wsl --version` and the `instanceIdleTimeout` / `vmIdleTimeout`
  settings in `%USERPROFILE%\.wslconfig`.

## 4. Add one build host per distribution in the IDE

Open **Window > Services > C/C++ Build Hosts > Add New Host...** and repeat for
each distribution:

1. **Hostname:** `127.0.0.1`, **Port:** the distribution's port, for example `2201`.
2. **User:** your Linux user in that distribution.
3. **Authentication:** choose **SSH Key File** and select
   `%USERPROFILE%\.ssh\id_ed25519_wsl`.
4. On **Summary**, set **Display Name** to the distribution, for example
   `WSL Ubuntu 24.04`. All the hosts are `127.0.0.1` and only the port tells them
   apart, so without a display name they look alike in every menu.
5. **Access project files via:** see step 5.

The IDE identifies a host by `user@host:port`, so a different port per
distribution means separate hosts, each with its own tool collections, host keys
and remote copies of the helpers (pty, killall, unbuffer; this build ships them
for Linux x86_64 and aarch64).

The first connection to each port asks you to accept that distribution's host
key. If you reinstall a distribution, its key changes. Remove the old
`[127.0.0.1]:<port>` line from `known_hosts` and accept the new key.

## 5. Where the project lives and how the files get there

Pick a mode per project in **Project Properties > Build > Build Host** and on
the host's **Access project files via** setting:

| Project lives in | Mode | Notes |
|---|---|---|
| The distribution (`~/src/app`) | **Remote Sources**: open it with File > Open Remote Project | Recommended. Builds run at full Linux speed, and there is no copying and no line-ending trouble. |
| Windows (`C:\work\app`) | **Automatic copying** or **SFTP** | The IDE copies the sources to the host before each build. Safe and works the same for every distribution. |
| Windows (`C:\work\app`) | **System level file sharing** with a manual path mapping | Builds directly on `/mnt/c`, with no copy but slow I/O. In **Host Properties > Path Mappings**, add `C:\work` → `/mnt/c/work`. The mapping is per host, so add it to every distribution that builds the project. |

Do not open `\\wsl.localhost\<Distro>\...` as a local project. The IDE treats it
as a Windows network path, and builds, the code model and file watching are slow
and unreliable there. Use Remote Sources instead.

## 6. Check each host

For every distribution:

1. **C/C++ Build Hosts > <host> > Properties**: the platform is Linux and a tool
   collection (GNU) is found.
2. Build and run a hello-world project on that host. The output must come from
   the right distribution: `cat /etc/os-release` in a run configuration, or
   `uname -a` in a terminal opened on the host.
3. Debug it once with gdb to confirm the pty and terminal work.

## Troubleshooting

| Symptom | Cause and fix |
|---|---|
| Host shows *No SSH Access* | The distribution is not running (step 3), or `sshd` is not on its port: run `ss -tlnp` inside it. |
| Connected to the wrong distribution | Two distributions use the same port, or Windows OpenSSH owns the port. Give every distribution a unique port other than 22. |
| `Connection refused` on `localhost` but `127.0.0.1` works | `localhost` resolved to IPv6. Use `127.0.0.1`. |
| Host key mismatch after reinstalling a distribution | Remove the `[127.0.0.1]:<port>` line from `known_hosts`. |
| Very slow builds | The project is on `/mnt/c`. Move it into the distribution and use Remote Sources. |
| `make` fails with `$'\r': command not found` | The scripts have Windows line endings from a checkout on Windows. Set `git config core.autocrlf input`, or keep the checkout inside the distribution. |
| No tool collection found | Install the compilers (step 2b), then use **Restore Defaults** in the host's tool collections. |

## Not covered yet

Native WSL support is an open task on the cnd branches, together with Cygwin,
MSYS2 and Git Bash terminals. It would start distributions with `wsl.exe -d`,
with no SSH, and map `\\wsl.localhost` paths automatically. Until that exists,
the SSH setup above is the supported route.
