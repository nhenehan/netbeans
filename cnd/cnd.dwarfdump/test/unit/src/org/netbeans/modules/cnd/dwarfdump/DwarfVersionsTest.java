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
package org.netbeans.modules.cnd.dwarfdump;

import java.io.File;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.Map;
import java.util.concurrent.TimeUnit;
import org.junit.Assume;
import org.netbeans.junit.NbTestCase;
import org.netbeans.modules.cnd.dwarfdump.dwarfconsts.LANG;
import org.netbeans.modules.cnd.dwarfdump.source.SourceFile;

/**
 * Reads binaries compiled with the installed gcc/clang in DWARF 4, DWARF 5 and
 * 64-bit DWARF. The binaries are built at test time, so nothing binary is
 * checked in; a compiler that is not installed skips its cases.
 */
public class DwarfVersionsTest extends NbTestCase {

    private static final String MAIN_C
            = "#include \"util.h\"\n" // NOI18N
            + "int square(int v) {\n" // NOI18N
            + "    return SQ(v);\n" // NOI18N
            + "}\n" // NOI18N
            + "int main(void) {\n" // NOI18N
            + "    return square(FOO);\n" // NOI18N
            + "}\n"; // NOI18N
    private static final String UTIL_H = "#define SQ(x) ((x)*(x))\nint square(int v);\n"; // NOI18N
    private static final String TWO_C = "#include \"util.h\"\nint twice(int v) {\n    return SQ(v) + v;\n}\n"; // NOI18N

    public DwarfVersionsTest(String name) {
        super(name);
    }

    public void testGccDwarf4() throws Exception {
        check("gcc", "-gdwarf-4"); // NOI18N
    }

    public void testGccDwarf5() throws Exception {
        check("gcc", "-gdwarf-5"); // NOI18N
    }

    public void testGccDwarf5With64BitOffsets() throws Exception {
        check("gcc", "-gdwarf-5", "-gdwarf64"); // NOI18N
    }

    public void testClangDwarf5() throws Exception {
        // clang uses DW_FORM_strx/addrx and file numbers starting at 0
        check("clang", "-gdwarf-5"); // NOI18N
    }

    private void check(String compiler, String... debugFlags) throws Exception {
        File dir = getWorkDir();
        write(new File(dir, "main.c"), MAIN_C); // NOI18N
        write(new File(dir, "two.c"), TWO_C); // NOI18N
        write(new File(dir, "util.h"), UTIL_H); // NOI18N
        File binary = new File(dir, "app"); // NOI18N
        List<String> command = new ArrayList<String>(Arrays.asList(compiler, "-g3", "-O0", "-DFOO=3")); // NOI18N
        command.addAll(Arrays.asList(debugFlags));
        command.addAll(Arrays.asList("main.c", "two.c", "-o", binary.getPath())); // NOI18N
        Assume.assumeTrue(compiler + " not available", compile(dir, command));

        List<SourceFile> units = new ArrayList<SourceFile>();
        for (SourceFile unit : CompileLineService.getSourceFileProperties(binary.getPath(), true)) {
            if ("main.c".equals(unit.getSourceFileName()) || "two.c".equals(unit.getSourceFileName())) { // NOI18N
                units.add(unit);
            }
        }
        assertEquals("both units of " + command, 2, units.size()); // NOI18N
        for (SourceFile unit : units) {
            assertTrue("C language: " + unit.getSourceLanguage(), LANG.isC(unit.getSourceLanguage())); // NOI18N
            assertEquals(dir.getAbsolutePath(), unit.getCompilationDir());
            String dwarf = unit.getDwarfDump();
            assertTrue("file table lists the unit: " + dwarf, dwarf.contains("'" + unit.getSourceFileName() + "'")); // NOI18N
            if (!"clang".equals(compiler)) {
                // gcc lists headers that only contribute macros (clang lists
                // only files with code, here just the unit with its MD5) and
                // records command line macros in .debug_macro
                assertTrue("file table lists util.h: " + dwarf, dwarf.contains("'util.h'")); // NOI18N
                assertTrue("-DFOO=3 recovered: " + dwarf, dwarf.contains("-DFOO='3'")); // NOI18N
            }
        }

        Map<String, Offset2LineService.AbstractFunctionToLine> functions = Offset2LineService.getOffset2Line(binary.getPath());
        assertNotNull("square has line info", functions.get("square")); // NOI18N
        assertNotNull("twice has line info", functions.get("twice")); // NOI18N
        Offset2LineService.SourceLineInfo square = functions.get("square").getLine(0); // NOI18N
        assertTrue("square in main.c: " + square, square.getFileName().endsWith("main.c")); // NOI18N
        assertEquals("square starts on line 2", 2, square.getLine()); // NOI18N
        Offset2LineService.SourceLineInfo twice = functions.get("twice").getLine(0); // NOI18N
        assertTrue("twice in two.c: " + twice, twice.getFileName().endsWith("two.c")); // NOI18N
        assertEquals("twice starts on line 2", 2, twice.getLine()); // NOI18N
    }

    private static void write(File file, String text) throws IOException {
        Files.write(file.toPath(), text.getBytes(StandardCharsets.UTF_8));
    }

    private static boolean compile(File dir, List<String> command) throws InterruptedException {
        try {
            Process process = new ProcessBuilder(command).directory(dir).redirectErrorStream(true).start();
            process.getInputStream().transferTo(System.err);
            return process.waitFor(60, TimeUnit.SECONDS) && process.exitValue() == 0;
        } catch (IOException ex) {
            return false; // compiler not installed
        }
    }
}
