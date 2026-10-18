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
import java.util.Set;
import org.netbeans.api.project.Project;
import org.netbeans.modules.cnd.discovery.projectimport.ImportExecutable;
import org.netbeans.modules.cnd.discovery.projectimport.ImportProject;
import org.netbeans.modules.cnd.makeproject.api.ui.wizard.IteratorExtension;
import org.netbeans.modules.cnd.makeproject.api.ui.wizard.WizardConstants;
import org.netbeans.modules.cnd.utils.CndPathUtilities;
import org.openide.WizardDescriptor;
import org.openide.filesystems.FileObject;
import org.openide.util.lookup.ServiceProvider;

/**
 * Trimmed implementation of {@link IteratorExtension} from the original
 * {@code cnd.discovery} module, without the code model and without build-log
 * and DWARF based configuration of the source files.
 * <ul>
 *   <li>{@link #createProject(WizardDescriptor)}: "C/C++ Project with Existing
 *       Sources", through {@link ImportProject}.</li>
 *   <li>{@link #discoverArtifacts(Map)}: the source root and shared libraries
 *       of a local binary, read from its debug information
 *       ({@link BinaryArtifacts}); for a remote binary its folder.</li>
 *   <li>{@link #discoverProject(Map, Project, ProjectKind)}: "C/C++ Project from
 *       Binary File" and the run dialog, through {@link ImportExecutable}.</li>
 *   <li>The code model methods do nothing; code assistance comes from clangd.</li>
 * </ul>
 * Original source: {@code contrib/cnd.discovery/src/.../wizard/DiscoveryExtension.java}
 * of the Apache NetBeans repository (commit d84e125315). Based on the trimmed
 * version by Piotr Hoppe for https://github.com/piotrhoppe/netbeans-cnd.
 */
@ServiceProvider(service = IteratorExtension.class)
public class DiscoveryExtension implements IteratorExtension {

    /** Creates a new instance of DiscoveryExtension */
    public DiscoveryExtension() {
    }

    @Override
    public Set<FileObject> createProject(WizardDescriptor wizard) throws IOException {
        return new ImportProject(wizard).create();
    }

    @Override
    public void discoverProject(Map<String, Object> map, Project lastSelectedProject, ProjectKind projectKind) {
        // ProjectKind only selected how much of the DWARF information was applied
        new ImportExecutable(map, lastSelectedProject);
    }

    @Override
    public void discoverArtifacts(Map<String, Object> map) {
        List<String> binaries = new ArrayList<>();
        binaries.add(WizardConstants.DISCOVERY_BUILD_RESULT.fromMap(map));
        String libraries = WizardConstants.DISCOVERY_LIBRARIES.fromMap(map);
        if (libraries != null) {
            for (String lib : libraries.split(";")) { // NOI18N
                if (!lib.trim().isEmpty()) {
                    binaries.add(lib.trim());
                }
            }
        }
        String binaryFolder = CndPathUtilities.getDirName(binaries.get(0));
        // null for local binaries; remote ones are copied for reading
        BinaryArtifacts artifacts = BinaryArtifacts.scan(binaries, WizardConstants.DISCOVERY_BINARY_FILESYSTEM.fromMap(map));
        WizardConstants.DISCOVERY_ROOT_FOLDER.toMap(map, artifacts.sourceRoot != null ? artifacts.sourceRoot : binaryFolder);
        WizardConstants.DISCOVERY_BINARY_DEPENDENCIES.toMap(map, artifacts.dlls);
        WizardConstants.DISCOVERY_BINARY_SEARCH_PATH.toMap(map, artifacts.searchPaths);
        WizardConstants.DISCOVERY_ERRORS.toMap(map, artifacts.errors);
    }

    @Override
    public void discoverHeadersByModel(Project project) {
    }

    @Override
    public void disableModel(Project project) {
    }
}
