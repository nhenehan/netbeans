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

//! Process exit codes; must match FSSExitCodes.java and the C exitcodes.h.

#![allow(dead_code)]

pub const FAILURE_LOCKING_MUTEX: i32 = 201;
pub const FAILURE_UNLOCKING_MUTEX: i32 = 202;
pub const WRONG_ARGUMENT: i32 = 203;
pub const FAILURE_GETTING_HOME_DIR: i32 = 204;
pub const FAILURE_CREATING_STORAGE_SUPER_DIR: i32 = 205;
pub const FAILURE_ACCESSING_STORAGE_SUPER_DIR: i32 = 206;
pub const FAILURE_CREATING_STORAGE_DIR: i32 = 207;
pub const FAILURE_ACCESSING_STORAGE_DIR: i32 = 208;
pub const FAILURE_CREATING_TEMP_DIR: i32 = 209;
pub const FAILURE_ACCESSING_TEMP_DIR: i32 = 210;
pub const FAILURE_CREATING_CACHE_DIR: i32 = 211;
pub const FAILURE_ACCESSING_CACHE_DIR: i32 = 212;
pub const NO_MEMORY_EXPANDING_DIRTAB: i32 = 213;
pub const FAILED_CHDIR: i32 = 214;
pub const FAILURE_OPENING_LOCK_FILE: i32 = 215;
pub const FAILURE_LOCKING_LOCK_FILE: i32 = 216;
pub const FAILURE_DIRTAB_DOUBLE_CACHE_OPEN: i32 = 217;
pub const DIRTAB_DOUBLE_INITIALIZATION: i32 = 218;
pub const DIRTAB_SET_PERSIST_DIR_AFTER_INITIALIZATION: i32 = 219;
pub const FAILURE_SETTING_SIGNAL_HANDLER: i32 = 221;
pub const FAILURE_SETTING_EXIT_FUNCTION: i32 = 222;
pub const FAILURE_ILLEGAL_ARGUMENT: i32 = 223;
pub const FAILURE_ALLOCATE_MEMORY: i32 = 224;
pub const FAILURE_REALLOCATE_MEMORY: i32 = 225;
