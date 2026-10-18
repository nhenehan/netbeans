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

package org.netbeans.modules.cnd.dwarfdump.dwarfconsts;

import java.util.HashMap;

/**
 *
 */
public enum LANG {
    DW_LANG_C89(0x0001, "C 89"), // NOI18N
    DW_LANG_C(0x0002, "C"), // NOI18N
    DW_LANG_Ada83(0x0003, "Ada 83"), // NOI18N
    DW_LANG_C_plus_plus(0x0004, "C++"), // NOI18N
    DW_LANG_Cobol74(0x0005, "Cobol 74"), // NOI18N
    DW_LANG_Cobol85(0x0006, "Cobol 85"), // NOI18N
    DW_LANG_Fortran77(0x0007, "Fortran 77"), // NOI18N
    DW_LANG_Fortran90(0x0008, "Fortran 90"), // NOI18N
    DW_LANG_Pascal83(0x0009, "Pascal 83"), // NOI18N
    DW_LANG_Modula2(0x000a, "Modula 2"), // NOI18N
    DW_LANG_Java(0x000b, "Java"), // NOI18N
    DW_LANG_C99(0x000c, "C 99"), // NOI18N
    DW_LANG_Ada95(0x000d, "Ada"), // NOI18N
    DW_LANG_Fortran95(0x000e, "Fortran 95"), // NOI18N
    DW_LANG_PLI(0x000f, "PL/I"), // NOI18N
    DW_LANG_ObjC(0x0010, "Objective-C"), // NOI18N
    DW_LANG_ObjC_plus_plus(0x0011, "Objective-C++"), // NOI18N
    DW_LANG_UPC(0x0012, "Unified Parallel C"), // NOI18N
    DW_LANG_D(0x0013, "D"), // NOI18N
    /* DWARF 4 and 5.  */
    DW_LANG_Python(0x0014, "Python"), // NOI18N
    DW_LANG_OpenCL(0x0015, "OpenCL"), // NOI18N
    DW_LANG_Go(0x0016, "Go"), // NOI18N
    DW_LANG_Modula3(0x0017, "Modula 3"), // NOI18N
    DW_LANG_Haskell(0x0018, "Haskell"), // NOI18N
    DW_LANG_C_plus_plus_03(0x0019, "C++ 03"), // NOI18N
    DW_LANG_C_plus_plus_11(0x001a, "C++ 11"), // NOI18N
    DW_LANG_OCaml(0x001b, "OCaml"), // NOI18N
    DW_LANG_Rust(0x001c, "Rust"), // NOI18N
    DW_LANG_C11(0x001d, "C 11"), // NOI18N
    DW_LANG_Swift(0x001e, "Swift"), // NOI18N
    DW_LANG_Julia(0x001f, "Julia"), // NOI18N
    DW_LANG_Dylan(0x0020, "Dylan"), // NOI18N
    DW_LANG_C_plus_plus_14(0x0021, "C++ 14"), // NOI18N
    DW_LANG_Fortran03(0x0022, "Fortran 03"), // NOI18N
    DW_LANG_Fortran08(0x0023, "Fortran 08"), // NOI18N
    DW_LANG_RenderScript(0x0024, "RenderScript"), // NOI18N
    DW_LANG_BLISS(0x0025, "BLISS"), // NOI18N
    /* DWARF language code registry (after DWARF 5).  */
    DW_LANG_C_plus_plus_17(0x002a, "C++ 17"), // NOI18N
    DW_LANG_C_plus_plus_20(0x002b, "C++ 20"), // NOI18N
    DW_LANG_C17(0x002c, "C 17"), // NOI18N
    DW_LANG_Fortran18(0x002d, "Fortran 18"), // NOI18N
    DW_LANG_lo_user(0x8000, null), // NOI18N
    DW_LANG_SUN_Assembler(0x9001, "Assembler"), // NOI18N
    DW_LANG_hi_user(0xffff, null); // NOI18N
    
/* What about dwarf 3.0 ?
    See http://dwarf.freestandards.org/Dwarf3Std.php
*/    
    
    private final int value;
    private final String name;
    static private final HashMap<Integer, LANG> hashmap = new HashMap<Integer, LANG>();
    
    static {
        for (LANG elem : values()) {
            hashmap.put(elem.value, elem);
        }
    }
    
    LANG(int value, String name) {
        this.value = value;
        this.name = name;
    }

    public static LANG get(int val) {
        return hashmap.get(val);
    }
    
    public int value() {
        return value;
    }
    
    @Override
    public String toString() {
        return name;
    }

    /** Whether a language name (as getSourceLanguage() returns it) is a C dialect. */
    public static boolean isC(String language) {
        return DW_LANG_C.toString().equals(language)
                || DW_LANG_C89.toString().equals(language)
                || DW_LANG_C99.toString().equals(language)
                || DW_LANG_C11.toString().equals(language)
                || DW_LANG_C17.toString().equals(language);
    }

    /** Whether a language name (as getSourceLanguage() returns it) is a C++ dialect. */
    public static boolean isCpp(String language) {
        return DW_LANG_C_plus_plus.toString().equals(language)
                || DW_LANG_C_plus_plus_03.toString().equals(language)
                || DW_LANG_C_plus_plus_11.toString().equals(language)
                || DW_LANG_C_plus_plus_14.toString().equals(language)
                || DW_LANG_C_plus_plus_17.toString().equals(language)
                || DW_LANG_C_plus_plus_20.toString().equals(language);
    }
}
