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

package org.netbeans.modules.cnd.dwarfdump.reader;

import java.io.IOException;
import org.netbeans.modules.cnd.dwarfdump.Magic;
import org.netbeans.modules.cnd.dwarfdump.dwarfconsts.ATE;
import org.netbeans.modules.cnd.dwarfdump.dwarfconsts.ATTR;
import org.netbeans.modules.cnd.dwarfdump.dwarfconsts.FORM;
import org.netbeans.modules.cnd.dwarfdump.dwarfconsts.LANG;
import org.netbeans.modules.cnd.dwarfdump.dwarfconsts.SECTIONS;
import org.netbeans.modules.cnd.dwarfdump.section.DwarfAbbriviationTableSection;
import org.netbeans.modules.cnd.dwarfdump.section.DwarfArangesSection;
import org.netbeans.modules.cnd.dwarfdump.section.DwarfAttribute;
import org.netbeans.modules.cnd.dwarfdump.section.DwarfDebugInfoSection;
import org.netbeans.modules.cnd.dwarfdump.section.DwarfLineInfoSection;
import org.netbeans.modules.cnd.dwarfdump.section.DwarfMacroInfoSection;
import org.netbeans.modules.cnd.dwarfdump.section.DwarfNameLookupTableSection;
import org.netbeans.modules.cnd.dwarfdump.section.DwarfRelaDebugInfoSection;
import org.netbeans.modules.cnd.dwarfdump.section.ElfSection;
import org.netbeans.modules.cnd.dwarfdump.section.StabIndexSection;
import org.netbeans.modules.cnd.dwarfdump.section.StabIndexStrSection;
import org.netbeans.modules.cnd.dwarfdump.section.StringTableSection;

/**
 *
 */
public class DwarfReader extends ElfReader {
    
    public DwarfReader(String fname, MyRandomAccessFile reader, Magic magic, long shift, long length) throws IOException {
        super(fname, reader, magic, shift, length);
        getSection(SECTIONS.DEBUG_STR); 
    }

    /**
     * A DWARF 5 value given as an index (DW_FORM_strx*, DW_FORM_addrx*): it can
     * only be resolved with the compilation unit's DW_AT_str_offsets_base or
     * DW_AT_addr_base, which CompilationUnit does once the unit's root entry
     * has been read.
     */
    public static final class IndexedValue {
        public final boolean string;
        public final long index;

        IndexedValue(boolean string, long index) {
            this.string = string;
            this.index = index;
        }

        @Override
        public String toString() {
            return (string ? "strx " : "addrx ") + index; // NOI18N
        }
    }

    private int dwarfVersion = 4;

    /** The version of the compilation unit being read; some forms depend on it. */
    public void setDwarfVersion(int dwarfVersion) {
        this.dwarfVersion = dwarfVersion;
    }

    public int getDwarfVersion() {
        return dwarfVersion;
    }

    /** Reads a section offset: 4 bytes in 32-bit DWARF, 8 in 64-bit DWARF. */
    private Number readOffset() throws IOException {
        return is32Bit() ? (Number) readInt() : (Number) readLong();
    }

    public Object readAttrValue(DwarfAttribute attr) throws IOException {
        Object value = attr.valueForm == FORM.DW_FORM_implicit_const
                ? (Object) Long.valueOf(attr.implicitConst)
                : readForm(attr.valueForm);

        if (attr.attrName == null) {
            return value;
        }

        if (attr.attrName.equals(ATTR.DW_AT_language)) {
            return LANG.get(((Number)value).intValue());
        }
        
        if (attr.attrName.equals(ATTR.DW_AT_encoding) && value instanceof Number) {
            return ATE.get(((Number)value).byteValue());
        }
        
        if (attr.attrName.equals(ATTR.DW_AT_decl_line) && value instanceof Number) {
            return Integer.valueOf(((Number)value).intValue());
        }
        
        return value;
    }
    
    public Object readForm(FORM form) throws IOException {
        if (form == null) {
            throw new IOException("unknown form"); // NOI18N
        }
        switch(form) {
            case DW_FORM_addr:
                return read(new byte[getAddressSize()]);
            case DW_FORM_block2:
                return read(new byte[ByteStreamReader.ushortToInt(readShort())]);
            case DW_FORM_block4:
                return read(new byte[readInt()]);
            case DW_FORM_data2:
                //TODO: check on all architectures!
                //return read(new byte[2]);
                return readShort();
            case DW_FORM_data4:
                //TODO: check on all architectures!
                //return read(new byte[4]);
                return readInt();
            case DW_FORM_data8:
                //TODO: check on all architectures!
                //return read(new byte[8]);
                return readLong();
            case DW_FORM_data16:
                return read(new byte[16]);
            case DW_FORM_sec_offset:
                return readOffset();
            case DW_FORM_string:
                return readString();
            case DW_FORM_exprloc:
            case DW_FORM_block:
                return read(new byte[readUnsignedLEB128()]);
            case DW_FORM_block1:
                return read(new byte[readUnsignedByte()]);
            case DW_FORM_data1:
                return readByte();
            case DW_FORM_flag:
                return readBoolean();
            case DW_FORM_sdata:
                return readSignedLEB128();
            case DW_FORM_strp:
                return ((StringTableSection)getSection(SECTIONS.DEBUG_STR)).getString(readOffset().longValue());
            case DW_FORM_line_strp: {
                long offset = readOffset().longValue();
                StringTableSection lineStr = (StringTableSection)getSection(SECTIONS.DEBUG_LINE_STR);
                return lineStr == null ? null : lineStr.getString(offset);
            }
            case DW_FORM_udata:
                return readUnsignedLEB128();
            case DW_FORM_ref_addr:
                // an offset since DWARF 3, an address in DWARF 2
                return read(new byte[dwarfVersion <= 2 ? getAddressSize() : (is32Bit() ? 4 : 8)]);
            case DW_FORM_ref1:
                return readUnsignedByte();
            case DW_FORM_ref2:
                return ByteStreamReader.ushortToInt(readShort());
            case DW_FORM_ref4:
                return readInt();
            case DW_FORM_ref8:
                return readLong();
            case DW_FORM_ref_udata:
                return readUnsignedLEB128();
            case DW_FORM_indirect:
                return readForm(FORM.get(readUnsignedLEB128()));
            case DW_FORM_flag_present:
                return true;
            case DW_FORM_sig8:
                return readLong();
            case DW_FORM_ref_sup4:
                return readInt();
            case DW_FORM_ref_sup8:
                return readLong();
            case DW_FORM_strp_sup:
            case DW_FORM_GNU_strp_alt:
            case DW_FORM_GNU_ref_alt:
                // in a supplementary object file, which is not read
                return readOffset();
            case DW_FORM_strx:
            case DW_FORM_GNU_str_index:
                return new IndexedValue(true, ByteStreamReader.uintToLong(readUnsignedLEB128()));
            case DW_FORM_strx1:
                return new IndexedValue(true, readNumber(1));
            case DW_FORM_strx2:
                return new IndexedValue(true, readNumber(2));
            case DW_FORM_strx3:
                return new IndexedValue(true, readNumber(3));
            case DW_FORM_strx4:
                return new IndexedValue(true, readNumber(4));
            case DW_FORM_addrx:
            case DW_FORM_GNU_addr_index:
                return new IndexedValue(false, ByteStreamReader.uintToLong(readUnsignedLEB128()));
            case DW_FORM_addrx1:
                return new IndexedValue(false, readNumber(1));
            case DW_FORM_addrx2:
                return new IndexedValue(false, readNumber(2));
            case DW_FORM_addrx3:
                return new IndexedValue(false, readNumber(3));
            case DW_FORM_addrx4:
                return new IndexedValue(false, readNumber(4));
            case DW_FORM_loclistx:
            case DW_FORM_rnglistx:
                return readUnsignedLEB128();
            case DW_FORM_implicit_const:
                // the value is in the abbreviation (see readAttrValue)
                return 0L;
            default:
            throw new IOException("unknown type " + form); // NOI18N
        }
    }

    /**
     * Resolves a DW_FORM_strx* index: .debug_str_offsets holds one offset into
     * .debug_str per index, starting at the unit's DW_AT_str_offsets_base.
     */
    public String resolveStringIndex(long strOffsetsBase, long index) throws IOException {
        Integer idx = getSectionIndex(SECTIONS.DEBUG_STR_OFFSETS);
        StringTableSection str = (StringTableSection)getSection(SECTIONS.DEBUG_STR);
        if (idx == null || str == null) {
            return null;
        }
        long currPos = getFilePointer();
        try {
            int offsetSize = is32Bit() ? 4 : 8;
            seek(getSectionHeader(idx).getSectionOffset() + strOffsetsBase + index * offsetSize);
            return str.getString(readOffset().longValue());
        } finally {
            seek(currPos);
        }
    }

    /**
     * Resolves a DW_FORM_addrx* index: .debug_addr holds one address per index,
     * starting at the unit's DW_AT_addr_base. Returned as the address bytes,
     * like DW_FORM_addr.
     */
    public byte[] resolveAddressIndex(long addrBase, long index) throws IOException {
        Integer idx = getSectionIndex(SECTIONS.DEBUG_ADDR);
        if (idx == null) {
            return null;
        }
        long currPos = getFilePointer();
        try {
            seek(getSectionHeader(idx).getSectionOffset() + addrBase + index * getAddressSize());
            return read(new byte[getAddressSize()]);
        } finally {
            seek(currPos);
        }
    }

    @Override
    ElfSection initSection(Integer sectionIdx, String sectionName) throws IOException {
        if (sectionName.equals(SECTIONS.DEBUG_STR)) {
            return new StringTableSection(this, sectionIdx);
        }

        if (sectionName.equals(SECTIONS.DEBUG_LINE_STR)) {
            return new StringTableSection(this, sectionIdx);
        }
        
        if (sectionName.equals(SECTIONS.DEBUG_ARANGES)) {
            return new DwarfArangesSection(this, sectionIdx);
        }
        
        if (sectionName.equals(SECTIONS.DEBUG_INFO)) {
            return new DwarfDebugInfoSection(this, sectionIdx);
        }

        if (sectionName.equals(SECTIONS.STAB_INDEXSTR)) {
            return new StabIndexStrSection(this, sectionIdx);
        }

        if (sectionName.equals(SECTIONS.STAB_INDEX)) {
            return new StabIndexSection(this, sectionIdx);
        }

        if (sectionName.equals(SECTIONS.RELA_DEBUG_INFO)) {
            return new DwarfRelaDebugInfoSection(this, sectionIdx);
        }
        
        if (sectionName.equals(SECTIONS.DEBUG_ABBREV)) {
            return new DwarfAbbriviationTableSection(this, sectionIdx);
        }
        
        if (sectionName.equals(SECTIONS.DEBUG_LINE)) {
            return new DwarfLineInfoSection(this, sectionIdx);
        }
        
        if (sectionName.equals(SECTIONS.DEBUG_MACINFO)) {
            return new DwarfMacroInfoSection(this, sectionIdx, false);
        }

        if (sectionName.equals(SECTIONS.DEBUG_MACRO)) {
            return new DwarfMacroInfoSection(this, sectionIdx, true);
        }
        
        if (sectionName.equals(SECTIONS.DEBUG_PUBNAMES)) {
            return new DwarfNameLookupTableSection(this, sectionIdx);
        }

        return null;
    }
}
