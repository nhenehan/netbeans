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
package org.netbeans.modules.cnd.makeproject.ui.wizards;

import org.junit.Test;
import static org.junit.Assert.*;
import org.netbeans.modules.cnd.makeproject.api.wizards.BuildSupport.BuildFile;

public class FpmScriptSupportTest {

    @Test
    public void testPackageNameWithAppFolder() {
        // what `fpm new fpmdemo` generates
        String manifest = "name = \"fpmdemo\"\n"
                + "version = \"0.1.0\"\n"
                + "license = \"license\"\n"
                + "[build]\n"
                + "auto-executables = true\n"
                + "[install]\n"
                + "library = false\n"
                + "[fortran]\n"
                + "implicit-typing = false\n";
        assertEquals("fpmdemo", FpmScriptSupport.executableName(manifest, true));
    }

    @Test
    public void testLibraryOnlyPackage() {
        assertNull(FpmScriptSupport.executableName("name = \"mylib\"\n[library]\nsource-dir = \"src\"\n", false));
    }

    @Test
    public void testExplicitExecutableWins() {
        String manifest = "name = 'pkg'\n"
                + "[[executable]]\n"
                + "name = \"tool\"\n"
                + "source-dir = \"prog\"\n"
                + "[[executable]]\n"
                + "name = \"other\"\n";
        assertEquals("tool", FpmScriptSupport.executableName(manifest, false));
    }

    @Test
    public void testNamesInOtherTablesAreIgnored() {
        String manifest = "# comment\n"
                + "name = \"pkg\"\n"
                + "[dependencies]\n"
                + "stdlib = { git = \"https://github.com/fortran-lang/stdlib\" }\n"
                + "[[test]]\n"
                + "name = \"runTests\"\n";
        assertEquals("pkg", FpmScriptSupport.executableName(manifest, true));
    }

    @Test
    public void testCommandsAndRecognition() {
        FpmScriptSupport support = new FpmScriptSupport();
        BuildFile file = support.scriptToBuildFile("/home/nbtest/fpmdemo/fpm.toml");
        assertNotNull(file);
        assertTrue(FpmScriptSupport.isFpm(file));
        assertEquals("fpm install --profile debug --prefix dist", file.getBuildCommandLine(null, "/home/nbtest/fpmdemo"));
        assertEquals("fpm clean --all", file.getCleanCommandLine("", "/home/nbtest/fpmdemo"));
        assertNull(support.scriptToBuildFile("/home/nbtest/fpmdemo/Makefile"));
    }
}
