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

package org.netbeans.modules.cnd.discovery.wizard;

import java.io.IOException;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.logging.Level;
import java.util.logging.Logger;
import org.netbeans.modules.cnd.dwarfdump.CompilationUnitInterface;
import org.netbeans.modules.cnd.dwarfdump.Dwarf;
import org.netbeans.modules.cnd.dwarfdump.exception.WrongFileFormatException;
import org.netbeans.modules.cnd.dwarfdump.reader.ElfReader.SharedLibraries;
import org.netbeans.modules.cnd.utils.CndPathUtilities;
import org.netbeans.modules.cnd.utils.cache.CndFileUtils;
import org.openide.util.NbBundle;

/**
 * What the "from binary" wizard step needs to know about a local binary: the
 * source root (the common folder of the compilation units in its debug
 * information) and the shared libraries it links against.
 * <p>
 * A light replacement for the DWARF discovery provider of the original
 * {@code cnd.dwarfdiscovery} module; it reads the binary with
 * {@code cnd.dwarfdump} and does not configure the project.
 */
final class BinaryArtifacts {

    private static final Logger LOG = Logger.getLogger(BinaryArtifacts.class.getName());

    final List<String> dlls = new ArrayList<>();
    final List<String> searchPaths = new ArrayList<>();
    final List<String> errors = new ArrayList<>();
    String sourceRoot;

    private BinaryArtifacts() {
    }

    static BinaryArtifacts scan(List<String> binaries) {
        BinaryArtifacts result = new BinaryArtifacts();
        // source folder -> number of compilation units in it
        TreeMap<String, Integer> folders = new TreeMap<>();
        for (String binary : binaries) {
            Dwarf dump = null;
            try {
                dump = new Dwarf(binary);
                try {
                    Dwarf.CompilationUnitIterator iterator = dump.iteratorCompilationUnits();
                    while (iterator.hasNext()) {
                        CompilationUnitInterface cu = iterator.next();
                        if (cu == null || cu.getSourceFileName() == null || cu.getSourceLanguage() == null) {
                            continue;
                        }
                        String path = cu.getSourceFileAbsolutePath();
                        if (path == null || !CndFileUtils.isExistingFile(path)) {
                            path = Dwarf.fileFinder(binary, path);
                            if (path == null || !CndFileUtils.isExistingFile(path)) {
                                continue;
                            }
                        }
                        String folder = CndPathUtilities.getDirName(path.replace('\\', '/'));
                        if (folder != null) {
                            folders.merge(folder, 1, Integer::sum);
                        }
                    }
                } catch (IOException ex) {
                    // cnd.dwarfdump does not read DWARF 5, the default of gcc 11 and clang 14;
                    // the wizard then proposes the binary's folder as the source root
                    LOG.log(Level.FINE, "Cannot read debug information of {0}: {1}", new Object[]{binary, ex.getMessage()}); // NOI18N
                }
                SharedLibraries pubNames = dump.readPubNames();
                for (String dll : pubNames.getDlls()) {
                    if (!result.dlls.contains(dll)) {
                        result.dlls.add(dll);
                    }
                }
                for (String path : pubNames.getPaths()) {
                    if (!result.searchPaths.contains(path)) {
                        result.searchPaths.add(path);
                    }
                }
            } catch (WrongFileFormatException ex) {
                result.errors.add(NbBundle.getMessage(BinaryArtifacts.class, "BinaryArtifacts.WrongFormat", binary)); // NOI18N
                LOG.log(Level.FINE, "Unsupported format of {0}: {1}", new Object[]{binary, ex.getMessage()}); // NOI18N
            } catch (IOException ex) {
                result.errors.add(NbBundle.getMessage(BinaryArtifacts.class, "BinaryArtifacts.IOException", binary, ex.getMessage())); // NOI18N
                LOG.log(Level.INFO, "Cannot read " + binary, ex); // NOI18N
            } finally {
                if (dump != null) {
                    dump.dispose();
                }
            }
        }
        result.sourceRoot = commonRoot(folders);
        return result;
    }

    /**
     * The deepest folder that contains every source folder, or the busiest
     * source folder when they only share a top-level folder such as
     * {@code /home}.
     */
    private static String commonRoot(TreeMap<String, Integer> folders) {
        String common = null;
        for (String folder : folders.keySet()) {
            common = common == null ? folder : commonPrefix(common, folder);
        }
        if (common != null && depth(common) >= 2) {
            return common;
        }
        String busiest = null;
        int max = 0;
        for (Map.Entry<String, Integer> entry : folders.entrySet()) {
            if (entry.getValue() > max) {
                max = entry.getValue();
                busiest = entry.getKey();
            }
        }
        return busiest;
    }

    private static String commonPrefix(String a, String b) {
        String[] as = a.split("/"); // NOI18N
        String[] bs = b.split("/"); // NOI18N
        StringBuilder buf = new StringBuilder();
        for (int i = 0; i < Math.min(as.length, bs.length) && as[i].equals(bs[i]); i++) {
            if (i > 0) {
                buf.append('/');
            }
            buf.append(as[i]);
        }
        return buf.length() == 0 && a.startsWith("/") ? "/" : buf.toString(); // NOI18N
    }

    private static int depth(String folder) {
        int depth = 0;
        for (String part : folder.split("/")) { // NOI18N
            if (!part.isEmpty()) {
                depth++;
            }
        }
        return depth;
    }
}
