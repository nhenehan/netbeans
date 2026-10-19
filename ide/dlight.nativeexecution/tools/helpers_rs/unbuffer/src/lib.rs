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

//! unbuffer.so: preloaded (LD_PRELOAD, see UnbufferSupport) into programs run
//! in the Output window, so that their stdout is not block-buffered on the
//! pipe and output shows up as it is written. Port of tools/unbuffer.
//!
//! The only things it uses are the host program's `setbuf` and `stdout`,
//! resolved at load time. It links no libc of its own and has no DT_NEEDED
//! entry, so the same file loads into glibc and musl programs (the C build
//! needed glibc's libc.so.6).

#![no_std]

use core::ffi::{c_char, c_void};

extern "C" {
    /// `FILE *stdout` of the program's libc
    static mut stdout: *mut c_void;
    fn setbuf(stream: *mut c_void, buf: *mut c_char);
}

/// Runs when the library is loaded, before the program's main.
extern "C" fn unbuffer_stdout() {
    // SAFETY: the dynamic linker runs .init_array after libc is initialised,
    // so stdout is a valid FILE*; setbuf(.., NULL) makes it unbuffered
    unsafe { setbuf(stdout, core::ptr::null_mut()) }
}

#[used]
#[link_section = ".init_array"]
static INIT: extern "C" fn() = unbuffer_stdout;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
