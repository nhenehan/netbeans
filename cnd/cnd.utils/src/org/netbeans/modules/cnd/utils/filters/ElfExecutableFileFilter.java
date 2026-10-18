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

package org.netbeans.modules.cnd.utils.filters;

import java.io.File;
import java.io.FileInputStream;
import java.io.IOException;
import java.io.InputStream;
import org.netbeans.modules.cnd.spi.utils.CndFileSystemProvider;
import org.netbeans.modules.cnd.utils.CndUtils;
import org.netbeans.modules.cnd.utils.cache.CndFileUtils;
import org.netbeans.modules.cnd.utils.FileAndFileObjectFilter;
import org.openide.filesystems.FileObject;
import org.openide.util.NbBundle;

public class ElfExecutableFileFilter extends FileAndFileObjectFilter {

    private static ElfExecutableFileFilter instance = null;

    public ElfExecutableFileFilter() {
	super();
    }

    public static ElfExecutableFileFilter getInstance() {
	if (instance == null) {
            instance = new ElfExecutableFileFilter();
        }
	return instance;
    }
    
    @Override
    public String getDescription() {
        return NbBundle.getMessage(ElfExecutableFileFilter.class, "FILECHOOSER_ELFEXECUTABLE_FILEFILTER"); // NOI18N
    }
    
    @Override
    protected boolean mimeAccept(File f) {
        FileObject fo = CndFileSystemProvider.toFileObject(f);
	return (fo == null) ? checkElfHeader(f) : checkElfHeader(fo);
    }

    @Override
    protected boolean mimeAccept(FileObject f) {
        return checkElfHeader(f);
    }


    /** Check if this file's header represents an elf executable */
    private boolean checkElfHeader(Object f) {
        CndUtils.assertTrue((f instanceof File) || (f instanceof FileObject));
        try (InputStream is = (f instanceof File)
                ? new FileInputStream((File) f)
                : CndFileUtils.getInputStream((FileObject) f, HEADER_BYTES)) {
            byte[] b = new byte[ELF64_HEADER_SIZE];
            int headerSize = readFully(is, b, 0, b.length);
            if (headerSize < 18 || b[0] != 0x7f || b[1] != (byte) 'E'
                    || b[2] != (byte) 'L' || b[3] != (byte) 'F') {
                return false;
            }
            boolean lsb = b[5] == 1; // ELFDATA2LSB=1
            int e_type = readInt(b, 16, 2, lsb);
            if (e_type == 2) { // ET_EXEC=2
                return true;
            }
            if (e_type != 3) { // ET_DYN=3
                return false;
            }
            // Position-independent executables (the gcc default on current
            // distributions) are ET_DYN like shared libraries; tell them apart
            // by the program interpreter (PT_INTERP) they request.
            boolean elf64 = b[4] == 2; // ELFCLASS64=2
            if (headerSize < (elf64 ? ELF64_HEADER_SIZE : ELF32_HEADER_SIZE)) {
                return false;
            }
            long phoff = elf64 ? readLong(b, 32, lsb) : readInt(b, 28, 4, lsb);
            int phentsize = readInt(b, elf64 ? 54 : 42, 2, lsb);
            int phnum = readInt(b, elf64 ? 56 : 44, 2, lsb);
            if (phoff < headerSize || phentsize < 4 || phnum == 0
                    || phoff + (long) phentsize * phnum > HEADER_BYTES) {
                return false;
            }
            long skip = phoff - headerSize;
            while (skip > 0) {
                long n = is.skip(skip);
                if (n <= 0) {
                    return false;
                }
                skip -= n;
            }
            byte[] ph = new byte[phentsize * phnum];
            if (readFully(is, ph, 0, ph.length) < ph.length) {
                return false;
            }
            for (int i = 0; i < phnum; i++) {
                if (readInt(ph, i * phentsize, 4, lsb) == 3) { // PT_INTERP=3
                    return true;
                }
            }
            return false;
        } catch (Exception e) {
            return false;
        }
    }

    private static final int ELF32_HEADER_SIZE = 52;
    private static final int ELF64_HEADER_SIZE = 64;
    // Remote file systems cache the first 4000 bytes of each file (MagicCache);
    // asking for more would download the whole binary. Program headers sit
    // right after the ELF header, well within that.
    private static final int HEADER_BYTES = 4000;

    private static int readFully(InputStream is, byte[] b, int offset, int length) throws IOException {
        int read = 0;
        while (read < length) {
            int n = is.read(b, offset + read, length - read);
            if (n <= 0) {
                break;
            }
            read += n;
        }
        return read;
    }

    private static int readInt(byte[] b, int offset, int size, boolean lsb) {
        int value = 0;
        for (int i = 0; i < size; i++) {
            int shift = lsb ? 8 * i : 8 * (size - 1 - i);
            value |= (b[offset + i] & 0xff) << shift;
        }
        return value;
    }

    private static long readLong(byte[] b, int offset, boolean lsb) {
        long low = readInt(b, offset + (lsb ? 0 : 4), 4, lsb) & 0xffffffffL;
        long high = readInt(b, offset + (lsb ? 4 : 0), 4, lsb) & 0xffffffffL;
        return (high << 32) | low;
    }

    @Override
    protected String[] getSuffixes() {
        return new String[]{};
    }
}
