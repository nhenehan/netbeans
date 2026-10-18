// Licensed to the Apache Software Foundation (ASF) under one
// or more contributor license agreements.  See the NOTICE file
// distributed with this work for additional information
// regarding copyright ownership.  The ASF licenses this file
// to you under the Apache License, Version 2.0 (the
// "License"); you may not use this file except in compliance
// with the License.  You may obtain a copy of the License at
//
//   http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing,
// software distributed under the License is distributed on an
// "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied.  See the License for the
// specific language governing permissions and limitations
// under the License.

//! `fs_server`: the remote side of the NetBeans remote file system
//! (`org.netbeans.modules.remote.impl.fs.server.FSSDispatcher`).
//!
//! A drop-in replacement for the original C fs_server (1.12.8): same command
//! line, same line protocol on stdin/stdout, same exit codes and the same on-disk
//! cache layout under `~/.netbeans/remotefs`.

mod cli;
mod dirtab;
mod exitcodes;
mod fsutil;
mod out;
mod protocol;
mod server;
mod settings;

fn main() {
    let opts = cli::parse(std::env::args_os());
    server::run(opts);
}
