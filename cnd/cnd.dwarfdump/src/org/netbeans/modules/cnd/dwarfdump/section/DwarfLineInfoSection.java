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

/*
 * DwarfLineInfostmt_list.java
 *
 *
 * To change this template, choose Tools | Template Manager
 * and open the template in the editor.
 */

package org.netbeans.modules.cnd.dwarfdump.section;

import java.io.ByteArrayOutputStream;
import org.netbeans.modules.cnd.dwarfdump.dwarf.DwarfStatementList;
import org.netbeans.modules.cnd.dwarfdump.reader.DwarfReader;
import java.io.IOException;
import java.io.PrintStream;
import java.nio.charset.Charset;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Set;
import org.netbeans.modules.cnd.dwarfdump.dwarfconsts.ElfConstants;
import org.netbeans.modules.cnd.dwarfdump.dwarfconsts.FORM;
import org.netbeans.modules.cnd.dwarfdump.dwarfconsts.LNE;
import org.netbeans.modules.cnd.dwarfdump.dwarfconsts.LNS;
import org.netbeans.modules.cnd.dwarfdump.reader.ByteStreamReader;

/**
 *
 */
public class DwarfLineInfoSection extends ElfSection {
    private HashMap<Long, DwarfStatementList> statementLists = new HashMap<Long, DwarfStatementList>();

    public DwarfLineInfoSection(DwarfReader reader, int sectionIdx) {
        super(reader, sectionIdx);
    }

    public DwarfStatementList getStatementList(long offset) throws IOException {
        Long lOffset = Long.valueOf(offset);
        DwarfStatementList statementList = statementLists.get(lOffset);

        if (statementList == null) {
            statementList = readStatementList(offset);
            statementLists.put(lOffset, statementList);
        }

        return statementList;
    }

    private DwarfStatementList readStatementList(long offset) throws IOException {
        reader.seek(header.getSectionOffset() + offset);

        DwarfStatementList stmt_list = new DwarfStatementList(offset);

        // a line table can be 32- or 64-bit DWARF regardless of the unit that
        // was read last, so take the offset size from its own length field
        int previousClass = reader.getFileClass();
        try {
            return readStatementList(stmt_list);
        } finally {
            if (previousClass != 0) { // 0: not set yet
                reader.setFileClass(previousClass);
            }
        }
    }

    private DwarfStatementList readStatementList(DwarfStatementList stmt_list) throws IOException {
        int length = reader.readInt();
        if (length == -1) {
            reader.setFileClass(ElfConstants.ELFCLASS64);
            stmt_list.total_length = reader.readLong();
        } else {
            reader.setFileClass(ElfConstants.ELFCLASS32);
            stmt_list.total_length = ByteStreamReader.uintToLong(length);
        }
        stmt_list.version = reader.readShort();
        if (stmt_list.version >= 5) {
            stmt_list.address_size = ByteStreamReader.ubyteToInt(reader.readByte());
            reader.readByte(); // segment_selector_size
        }
        stmt_list.prologue_length = reader.read3264();
        // the line number program starts right after the header
        stmt_list.program_offset = reader.getFilePointer() - header.getSectionOffset() + stmt_list.prologue_length;
        stmt_list.minimum_instruction_length = ByteStreamReader.ubyteToInt(reader.readByte());
        if (stmt_list.version >= 4) {
            stmt_list.maximum_operations_per_instruction = ByteStreamReader.ubyteToInt(reader.readByte());
        }
        stmt_list.default_is_stmt = ByteStreamReader.ubyteToInt(reader.readByte());
        stmt_list.line_base = reader.readByte();
        stmt_list.line_range = ByteStreamReader.ubyteToInt(reader.readByte());
        stmt_list.opcode_base = ByteStreamReader.ubyteToInt(reader.readByte());

        stmt_list.standard_opcode_lengths = new long[stmt_list.opcode_base - 1];

        for (int i = 0; i < stmt_list.opcode_base - 1; i++) {
            stmt_list.standard_opcode_lengths[i] = reader.readUnsignedLEB128();
        }

        if (stmt_list.version >= 5) {
            readEntryTables(stmt_list);
            return stmt_list;
        }

        String dirname = reader.readString();

        while (dirname.length() > 0) {
            stmt_list.includeDirs.add(dirname);
            dirname = reader.readString();
        }

        String fname = reader.readString();

        while(fname.length() > 0) {
            stmt_list.fileEntries.add(new FileEntry(fname, reader.readUnsignedLEB128(), reader.readUnsignedLEB128(), reader.readUnsignedLEB128()));
            fname = reader.readString();
        }
        return stmt_list;
    }

    // DWARF 5 line number header content types (DW_LNCT_*)
    private static final int DW_LNCT_path = 0x1;
    private static final int DW_LNCT_directory_index = 0x2;
    private static final int DW_LNCT_timestamp = 0x3;
    private static final int DW_LNCT_size = 0x4;

    /**
     * Reads the DWARF 5 directory and file name tables, which describe their
     * entries with (content type, form) pairs. Directory 0 is the compilation
     * directory and is left out of includeDirs, so directory indices keep the
     * meaning they had before DWARF 5 (0 = compilation directory). File 0 is
     * the primary source file and is kept: DWARF 5 file numbers start at 0.
     */
    private void readEntryTables(DwarfStatementList stmt_list) throws IOException {
        stmt_list.fileIndexBase = 0;
        List<Object[]> directories = readEntryTable();
        for (int i = 1; i < directories.size(); i++) {
            Object path = directories.get(i)[DW_LNCT_path];
            stmt_list.includeDirs.add(path == null ? "" : path.toString()); // NOI18N
        }
        for (Object[] file : readEntryTable()) {
            Object path = file[DW_LNCT_path];
            stmt_list.fileEntries.add(new FileEntry(path == null ? "" : path.toString(), // NOI18N
                    toInt(file[DW_LNCT_directory_index]), toLong(file[DW_LNCT_timestamp]), toLong(file[DW_LNCT_size])));
        }
    }

    /** Reads one entry format description and the entries it describes. */
    private List<Object[]> readEntryTable() throws IOException {
        int formatCount = ByteStreamReader.ubyteToInt(reader.readByte());
        int[] contentTypes = new int[formatCount];
        FORM[] forms = new FORM[formatCount];
        for (int i = 0; i < formatCount; i++) {
            contentTypes[i] = reader.readUnsignedLEB128();
            forms[i] = FORM.get(reader.readUnsignedLEB128());
        }
        int count = reader.readUnsignedLEB128();
        List<Object[]> entries = new ArrayList<Object[]>(count);
        for (int e = 0; e < count; e++) {
            Object[] entry = new Object[DW_LNCT_size + 1];
            for (int i = 0; i < formatCount; i++) {
                Object value = ((DwarfReader) reader).readForm(forms[i]);
                if (contentTypes[i] >= DW_LNCT_path && contentTypes[i] <= DW_LNCT_size) {
                    entry[contentTypes[i]] = value; // DW_LNCT_MD5 and vendor types are skipped
                }
            }
            entries.add(entry);
        }
        return entries;
    }

    private static int toInt(Object value) {
        return (int) toLong(value);
    }

    private static long toLong(Object value) {
        if (value instanceof Byte) {
            return 0xFFL & (Byte) value;
        } else if (value instanceof Short) {
            return 0xFFFFL & (Short) value;
        } else if (value instanceof Integer) {
            return 0xFFFFFFFFL & (Integer) value;
        } else if (value instanceof Number) {
            return ((Number) value).longValue();
        }
        return 0;
    }

    @Override
    public void dump(PrintStream out) {
        super.dump(out);
        for (DwarfStatementList statementList : statementLists.values()) {
            statementList.dump(out);
        }
    }

    @Override
    public String toString() {
        try {
            ByteArrayOutputStream st = new ByteArrayOutputStream();
            PrintStream out = new PrintStream(st, false, "UTF-8"); // NOI18N
            dump(out);
            return st.toString("UTF-8"); //NOI18N
        } catch (IOException ex) {
            return ""; // NOI18N
        }
    }

    public LineNumber getLineNumber(long shift, long target) throws IOException{
        long currPos = reader.getFilePointer();
        try {
            DwarfStatementList statementList = getStatementList(shift);
            reader.seek(header.getSectionOffset() + statementList.program_offset);
            Set<LineNumber> res = interpret(statementList, shift, target);
            if (!res.isEmpty()) {
                return res.iterator().next();
            }
            return null;
        } finally {
            reader.seek(currPos);
        }
    }

    public Set<LineNumber> getLineNumbers(long shift) throws IOException{
        long currPos = reader.getFilePointer();
        try {
            DwarfStatementList statementList = getStatementList(shift);
            reader.seek(header.getSectionOffset() + statementList.program_offset);
            return interpret(statementList, shift, 0);
        } finally {
            reader.seek(currPos);
        }
    }

    private Set<LineNumber> interpret(DwarfStatementList section, long shift, long target) throws IOException {
        long address = 0;
        long base_address = 0;
        long prev_base_address = 0;
        String define_file = null;
        int fileno = 0;
        int lineno = 1;
        int prev_fileno = 0;
        int prev_lineno = 1;
        final int const_pc_add = 245 / section.line_range * section.minimum_instruction_length;
        int lineNumber = -1;
        int fileNumber = -1;
        String sourceFile = null;
        Set<LineNumber> result = new HashSet<LineNumber>();

        while (reader.getFilePointer() < header.getSectionOffset() + shift + section.total_length) {

            int opcode = ByteStreamReader.ubyteToInt(reader.readByte());

            if (opcode < section.opcode_base) {
                switch (LNS.get(opcode)) {
                    case DW_LNS_extended_op: {
                        int insn_len = reader.readUnsignedLEB128();
                        opcode = reader.readByte();

                        switch (LNE.get(opcode)) {
                            case DW_LNE_end_sequence:
                                lineNumber = prev_lineno;
                                sourceFile = (section.hasFile(prev_fileno + 1) ? section.getFilePath(prev_fileno + 1) : define_file);
                                if (sourceFile != null) {
                                    if (target > 0) {
                                        if (target >= prev_base_address && target < address) {
                                            LineNumber res = new LineNumber(sourceFile, lineNumber, prev_base_address, address);
                                            result.add(res);
                                            return result;
                                        }
                                    } else {
                                        LineNumber res = new LineNumber(sourceFile, lineNumber, prev_base_address, address);
                                        result.add(res);
                                    }
                                }
                                prev_lineno = lineno = 1;
                                prev_fileno = fileno = 0;
                                base_address = address = 0;
                                break;

                            case DW_LNE_set_address:
                                prev_base_address = base_address;
                                if (insn_len == 9) {
                                    base_address = reader.readLong();
                                } else if (insn_len == 5) {
                                    base_address = reader.readInt();
                                }
                                address = base_address;
                                if (prev_base_address == 0) {
                                    prev_base_address = base_address;
                                }
                                break;

                            case DW_LNE_define_file:
                                define_file = reader.readString();
                                reader.readUnsignedLEB128();
                                reader.readUnsignedLEB128();
                                reader.readUnsignedLEB128();
                                break;

                            default:
                                reader.seek(reader.getFilePointer() + insn_len);
                                break;
                        }
                        break;
                    }
                    case DW_LNS_copy:
                        lineNumber = prev_lineno == 1 ? lineno : prev_lineno;
                        fileNumber = prev_fileno == 0 ? fileno : prev_fileno;
                        sourceFile = (section.hasFile(fileNumber + 1) ? section.getFilePath(fileNumber + 1) : define_file);
                        if (sourceFile != null) {
                            if (target > 0) {
                                if (target >= prev_base_address && target < address) {
                                    LineNumber res = new LineNumber(sourceFile, lineNumber, prev_base_address, address);
                                    result.add(res);
                                    return result;
                                }
                            } else {
                                LineNumber res = new LineNumber(sourceFile, lineNumber, prev_base_address, address);
                                result.add(res);
                            }
                        }
                        prev_lineno = lineno;
                        prev_fileno = fileno;
                        break;

                    case DW_LNS_advance_pc:
                        {
                            long amt = reader.readUnsignedLEB128();
                            address += amt * section.minimum_instruction_length;
                        }
                        break;

                    case DW_LNS_advance_line:
                        {
                            long amt = reader.readSignedLEB128();
                            prev_lineno = lineno;
                            lineno += (int) amt;
                        }
                        break;

                    case DW_LNS_set_file:
                        prev_fileno = fileno;
                        fileno = (reader.readUnsignedLEB128() - 1);
                        break;

                    case DW_LNS_set_column:
                        reader.readUnsignedLEB128();
                        break;

                    case DW_LNS_negate_stmt:
                        break;

                    case DW_LNS_set_basic_block:
                        break;

                    case DW_LNS_const_add_pc:
                        address += const_pc_add;
                        break;

                    case DW_LNS_fixed_advance_pc:
                        {
                            int amt = ByteStreamReader.ushortToInt(reader.readShort());
                            address += amt;
                        }
                        break;
                }
            } else {
                int adj = (opcode & 0xFF) - section.opcode_base;
                int addr_adv = adj / section.line_range * section.minimum_instruction_length;
                int line_adv = section.line_base + (adj % section.line_range);
                long new_addr = address + addr_adv;
                int new_line = lineno + line_adv;
                sourceFile = (section.hasFile(prev_fileno + 1) ? section.getFilePath(prev_fileno + 1) : define_file);
                if (sourceFile != null) {
                    if (target > 0) {
                        if (target >= prev_base_address && target < new_addr) {
                            LineNumber res = new LineNumber(sourceFile, lineno, prev_base_address, new_addr);
                            result.add(res);
                            return result;
                        }
                    } else {
                        LineNumber res = new LineNumber(sourceFile, lineno, prev_base_address, new_addr);
                        result.add(res);
                    }
                }

                prev_base_address = new_addr;
                prev_lineno = lineno;
                prev_fileno = fileno;
                lineno = new_line;
                address = new_addr;
            }
        }
        if (prev_base_address != 0 && address != 0 && sourceFile != null) {
            if (target > 0) {
                if (target >= prev_base_address && target < address) {
                    LineNumber res = new LineNumber(sourceFile, lineno, prev_base_address, address);
                    result.add(res);
                    return result;
                }
            } else {
                LineNumber res = new LineNumber(sourceFile, lineno, prev_base_address, address);
                result.add(res);
            }
        }
        return result;
    }

    public static final class LineNumber implements Comparable<LineNumber> {
        public final String file;
        public final int line;
        public final long startOffset;
        public final long endOffset;
        private LineNumber(String file, int line, long startOffset, long endOffset){
            assert file != null;
            this.file = file;
            this.line = line;
            this.startOffset = startOffset;
            this.endOffset = endOffset;
        }

        @Override
        public boolean equals(Object obj) {
            if (obj instanceof LineNumber) {
                LineNumber other = (LineNumber) obj;
                return file.equals(other.file) && line == other.line && startOffset == other.endOffset && endOffset == other.endOffset;
            }
            return false;
        }

        @Override
        public int hashCode() {
            int hash = 7;
            hash = 67 * hash + (this.file != null ? this.file.hashCode() : 0);
            hash = 67 * hash + this.line;
            hash = 67 * hash + (int)(this.startOffset & 0xFFFFFFFF);
            hash = 67 * hash + (int)(this.endOffset & 0xFFFFFFFF);
            return hash;
        }

        @Override
        public String toString() {
            return file+":"+line+"\t(0x"+Long.toHexString(startOffset)+"-0x"+Long.toHexString(endOffset)+")"; // NOI18N
        }

        @Override
        public int compareTo(LineNumber o) {
            int res = file.compareTo(o.file);
            if (res == 0) {
                res = (int) ((startOffset - o.startOffset) & 0xFFFFFFFF);
            }
            if (res == 0) {
                res = (int) ((endOffset - o.endOffset) & 0xFFFFFFFF);
            }
            return res;
        }
    }
}

