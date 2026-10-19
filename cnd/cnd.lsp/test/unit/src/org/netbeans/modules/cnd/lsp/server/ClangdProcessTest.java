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
package org.netbeans.modules.cnd.lsp.server;

import java.util.Arrays;
import java.util.Collections;
import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNull;
import org.junit.Test;

public class ClangdProcessTest {

    @Test
    public void testQueryDriverJoinsUniquePaths() {
        assertEquals("--query-driver=/usr/bin/gcc,/usr/bin/g++,/opt/cross/bin/arm-gcc",
                ClangdProcess.queryDriverArgument(Arrays.asList(
                        "/usr/bin/gcc", "/usr/bin/g++", "/usr/bin/gcc", "", null,
                        "/opt/cross/bin/arm-gcc")));
    }

    @Test
    public void testQueryDriverSkipsPathsWithCommas() {
        assertEquals("--query-driver=/usr/bin/g++",
                ClangdProcess.queryDriverArgument(Arrays.asList("/odd,dir/g++", "/usr/bin/g++")));
    }

    @Test
    public void testNoQueryDriverWithoutCompilers() {
        assertNull(ClangdProcess.queryDriverArgument(Collections.emptyList()));
        assertNull(ClangdProcess.queryDriverArgument(Arrays.asList("", null)));
    }
}
