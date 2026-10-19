/*
 * Licensed to the Apache Software Foundation (ASF) under one
 * or more contributor license agreements.  See the NOTICE file
 * distributed with this work for additional information
 * regarding copyright ownership.  The ASF licenses this file
 * to you under the Apache License, Version 2.0 (the
 * "License"); you may not use this file except in compliance
 * with the License.  You may obtain a copy of the License at
 *
 *   http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing,
 * software distributed under the License is distributed on an
 * "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
 * KIND, either express or implied.  See the License for the
 * specific language governing permissions and limitations
 * under the License.
 */
package org.netbeans.modules.nativeexecution.support.hostinfo.impl;

import static org.junit.Assert.assertEquals;
import org.junit.Test;
import org.netbeans.modules.nativeexecution.api.HostInfo.CpuFamily;

/**
 * The CPU family mapping of hostinfo.sh's CPUFAMILY values.
 */
public class HostInfoFactoryTest {

    @Test
    public void testCpuFamilies() {
        assertEquals(CpuFamily.X86, HostInfoFactory.toCpuFamily("x86"));
        assertEquals(CpuFamily.SPARC, HostInfoFactory.toCpuFamily("sparc"));
        assertEquals(CpuFamily.AARCH64, HostInfoFactory.toCpuFamily("aarch64"));
        assertEquals(CpuFamily.RISCV64, HostInfoFactory.toCpuFamily("riscv64"));
        // 32-bit ARM reports uname -m, e.g. armv7l; macOS arm64 is mapped to "arm"
        assertEquals(CpuFamily.ARM, HostInfoFactory.toCpuFamily("armv7l"));
        assertEquals(CpuFamily.ARM, HostInfoFactory.toCpuFamily("arm"));
        assertEquals(CpuFamily.UNKNOWN, HostInfoFactory.toCpuFamily("mips64"));
    }
}
