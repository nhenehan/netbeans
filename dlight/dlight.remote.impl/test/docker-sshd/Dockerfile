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

# Throwaway remote host for the dlight/cnd remote tests (see README.md next to this file).
# Has what remote file system, remote build and remote debug need: sshd, a C/C++
# toolchain, gdb, make/cmake, gfortran/fpm, git/mercurial/subversion for the remote VCS modules,
# and a JRE for the Java "agent" that FileOperationsTestCase runs on the remote side.
FROM docker.io/library/debian:bookworm-slim

RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        openssh-server ca-certificates \
        build-essential gdb make cmake gfortran curl \
        git mercurial subversion \
        procps file less unzip zip \
        openjdk-17-jre-headless \
 && rm -rf /var/lib/apt/lists/* \
 && mkdir -p /run/sshd \
 && cp -a "/usr/include/$(gcc -print-multiarch)/sys" /usr/include/sys

# fpm (Fortran package manager) is not packaged for bookworm; use the upstream
# release binary, which is built with gcc 12 like the toolchain above
ARG FPM_VERSION=0.13.0
ARG FPM_SHA256=2e50a8649ac4136b9cf6b63c5d1b01adf26c02eedc18f32e16261c0ce655d46d
RUN curl -fsSL -o /usr/local/bin/fpm \
        "https://github.com/fortran-lang/fpm/releases/download/v${FPM_VERSION}/fpm-${FPM_VERSION}-linux-x86_64-gcc-12" \
 && echo "${FPM_SHA256}  /usr/local/bin/fpm" | sha256sum -c - \
 && chmod 0755 /usr/local/bin/fpm

ARG TEST_USER=nbtest
ARG TEST_PASSWORD=nbtest
RUN useradd -m -s /bin/bash "$TEST_USER" \
 && echo "$TEST_USER:$TEST_PASSWORD" | chpasswd \
 && sed -i 's/^#\?PasswordAuthentication .*/PasswordAuthentication yes/' /etc/ssh/sshd_config \
 && echo 'MaxSessions 50' >> /etc/ssh/sshd_config \
 && echo 'MaxStartups 50:30:100' >> /etc/ssh/sshd_config

EXPOSE 22
CMD ["/usr/sbin/sshd", "-D", "-e"]
