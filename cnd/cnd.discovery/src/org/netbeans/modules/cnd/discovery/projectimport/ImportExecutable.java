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
package org.netbeans.modules.cnd.discovery.projectimport;

import java.beans.PropertyChangeEvent;
import java.beans.PropertyChangeListener;
import java.io.IOException;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.logging.Level;
import java.util.logging.Logger;
import org.netbeans.api.progress.ProgressHandle;
import org.netbeans.api.project.Project;
import org.netbeans.api.project.ui.OpenProjects;
import org.netbeans.modules.cnd.api.remote.RemoteFileUtil;
import org.netbeans.modules.cnd.api.toolchain.CompilerSet;
import org.netbeans.modules.cnd.makeproject.api.MakeProjectOptions;
import org.netbeans.modules.cnd.makeproject.api.ProjectSupport;
import org.netbeans.modules.cnd.makeproject.api.SourceFolderInfo;
import org.netbeans.modules.cnd.makeproject.api.configurations.ConfigurationDescriptorProvider;
import org.netbeans.modules.cnd.makeproject.api.configurations.Folder;
import org.netbeans.modules.cnd.makeproject.api.configurations.MakeConfiguration;
import org.netbeans.modules.cnd.makeproject.api.configurations.MakeConfigurationDescriptor;
import org.netbeans.modules.cnd.makeproject.api.ui.wizard.WizardConstants;
import org.netbeans.modules.cnd.makeproject.api.wizards.ProjectGenerator;
import org.netbeans.modules.cnd.utils.CndPathUtilities;
import org.netbeans.modules.cnd.utils.FSPath;
import org.netbeans.modules.cnd.utils.cache.CndFileUtils;
import org.netbeans.modules.nativeexecution.api.ExecutionEnvironment;
import org.netbeans.modules.remote.spi.FileSystemProvider;
import org.openide.filesystems.FileObject;
import org.openide.filesystems.FileSystem;
import org.openide.util.Exceptions;
import org.openide.util.NbBundle;
import org.openide.util.RequestProcessor;

/**
 * Creates a MakeProject for an existing binary ("C/C++ Project from Binary
 * File"), or adds the binary's sources to a project that was just created for
 * it (the run dialog).
 * <p>
 * Trimmed version of the original {@code cnd.discovery} {@code ImportExecutable}.
 * Project creation is kept. The analysis that followed it (configuring every
 * source file from the DWARF information, adding dependent libraries as
 * projects, and opening {@code main} through the code model) depends on
 * {@code cnd.dwarfdiscovery} and the code model, so it is not included.
 * <p>
 * Original source: {@code contrib/cnd.discovery/src/.../projectimport/ImportExecutable.java}
 * of the Apache NetBeans repository.
 */
public final class ImportExecutable implements PropertyChangeListener {

    private static final Logger LOG = Logger.getLogger(ImportExecutable.class.getName());
    private static final RequestProcessor RP = new RequestProcessor(ImportExecutable.class.getName(), 2);
    private final Map<String, Object> map;
    private Project lastSelectedProject;
    private String sourcesPath;
    private boolean addSourceRoot;

    public ImportExecutable(Map<String, Object> map, Project lastSelectedProject) {
        this.map = map;
        this.lastSelectedProject = lastSelectedProject;
        if (lastSelectedProject == null) {
            postCreateProject();
        } else {
            addSourceRoot = true;
            sourcesPath = WizardConstants.DISCOVERY_ROOT_FOLDER.fromMap(map);
            RP.post(this::process);
        }
    }

    private void postCreateProject() {
        RP.post(() -> {
            ProgressHandle progress = ProgressHandle.createHandle(NbBundle.getMessage(ImportExecutable.class, "ImportExecutable.Progress.ProjectCreating")); // NOI18N
            progress.start();
            try {
                createProject();
            } catch (Throwable ex) {
                Exceptions.printStackTrace(ex);
            } finally {
                progress.finish();
            }
        });
    }

    private void createProject() {
        String binaryPath = WizardConstants.PROPERTY_BUILD_RESULT.fromMap(map);
        List<String> libs = new ArrayList<>();
        {
            String firstBinary = null;
            for (String s : binaryPath.split(";")) { //NOI18N
                String path = s.trim();
                if (path.isEmpty()) {
                    continue;
                }
                if (firstBinary == null) {
                    firstBinary = path;
                } else {
                    libs.add(s);
                }
            }
            binaryPath = firstBinary;
        }
        ExecutionEnvironment fullRemoteEnv = WizardConstants.PROPERTY_REMOTE_FILE_SYSTEM_ENV.fromMap(map);
        FileSystem fullRemotefileSystem = null;
        if (fullRemoteEnv != null) {
            fullRemotefileSystem = FileSystemProvider.getFileSystem(fullRemoteEnv);
        }
        sourcesPath = WizardConstants.PROPERTY_SOURCE_FOLDER_PATH.fromMap(map);
        FSPath projectFolder = WizardConstants.PROPERTY_PROJECT_FOLDER.fromMap(map);
        if (projectFolder == null) {
            // the wizard always sets it; the original fell back to the default project folder
            LOG.log(Level.WARNING, "No project folder for {0}", binaryPath); // NOI18N
            return;
        }
        String projectName = WizardConstants.PROPERTY_NAME.fromMap(map);
        projectFolder = new FSPath(projectFolder.getFileSystem(), RemoteFileUtil.normalizeAbsolutePath(projectFolder.getPath(), FileSystemProvider.getExecutionEnvironment(projectFolder.getFileSystem())));
        String baseDir = projectFolder.getPath();
        FileSystem sourceFileSystem = projectFolder.getFileSystem();
        if (projectName == null) {
            projectName = CndPathUtilities.getBaseName(baseDir);
        }
        String hostUID = WizardConstants.PROPERTY_HOST_UID.fromMap(map);
        CompilerSet toolchain = WizardConstants.PROPERTY_TOOLCHAIN.fromMap(map);
        boolean defaultToolchain = Boolean.TRUE.equals(WizardConstants.PROPERTY_TOOLCHAIN_DEFAULT.fromMap(map));
        MakeConfiguration conf = MakeConfiguration.createMakefileConfiguration(projectFolder, "Default",  hostUID, toolchain, defaultToolchain); // NOI18N
        String workingDirRel = ProjectSupport.toProperPath(new FSPath(sourceFileSystem, baseDir),  sourcesPath, MakeProjectOptions.getPathMode()); // it's better to pass project source mode here (once full remote is supprted here)
        workingDirRel = CndPathUtilities.naturalizeSlashes(sourceFileSystem, workingDirRel);
        conf.getMakefileConfiguration().getBuildCommandWorkingDir().setValue(workingDirRel);
        // Executable
        String exe = binaryPath;
        if (fullRemotefileSystem != null) {
            exe = CndPathUtilities.toRelativePath(CndPathUtilities.naturalizeSlashes(fullRemotefileSystem, baseDir), exe);
            exe = CndPathUtilities.normalizeSlashes(exe);
        } else {
            exe = CndPathUtilities.toRelativePath(CndPathUtilities.naturalizeSlashes(baseDir), exe);
            exe = CndPathUtilities.normalizeSlashes(exe);
        }
        conf.getMakefileConfiguration().getOutput().setValue(exe);
        String exePath = CndPathUtilities.getDirName(binaryPath);
        if (fullRemotefileSystem != null) {
            exePath = CndPathUtilities.toRelativePath(CndPathUtilities.naturalizeSlashes(fullRemotefileSystem, baseDir), exePath);
            exePath = CndPathUtilities.normalizeSlashes(exePath);
        } else {
            exePath = CndPathUtilities.toRelativePath(CndPathUtilities.naturalizeSlashes(baseDir), exePath);
            exePath = CndPathUtilities.normalizeSlashes(exePath);
        }
        conf.getProfile().setRunDirectory(exePath);
        conf.getProfile().setBuildFirst(false);
        conf.getCodeAssistanceConfiguration().getResolveSymbolicLinks().setValue(MakeProjectOptions.getResolveSymbolicLinks());

        ArrayList<String> importantBinaries = new ArrayList<>();
        importantBinaries.add(binaryPath);
        importantBinaries.addAll(libs);
        {
            String launcher = projectFolder.getPath()+"/nbproject/private/launcher.properties"; //NOI18N
            launcher = ProjectSupport.toProperPath(projectFolder, CndPathUtilities.naturalizeSlashes(sourceFileSystem, launcher), MakeProjectOptions.getPathMode());
            launcher = CndPathUtilities.normalizeSlashes(launcher);
            importantBinaries.add(launcher);
        }

        ProjectGenerator.ProjectParameters prjParams = new ProjectGenerator.ProjectParameters(projectName, projectFolder);
        prjParams.setOpenFlag(false)
                 .setConfiguration(conf)
                 .setImportantFiles(importantBinaries.iterator())
                 .setMakefileName(""); //NOI18N
        Boolean trueSourceRoot = WizardConstants.PROPERTY_TRUE_SOURCE_ROOT.fromMap(map);
        final FileSystem rootFileSystem = fullRemotefileSystem != null ? fullRemotefileSystem : sourceFileSystem;
        if (trueSourceRoot != null && trueSourceRoot && sourcesPath != null && !sourcesPath.isEmpty()) {
            List<SourceFolderInfo> list = new ArrayList<>();
            list.add(new SourceFolderInfo() {

                @Override
                public FileObject getFileObject() {
                    return CndFileUtils.toFileObject(rootFileSystem, CndFileUtils.normalizeAbsolutePath(rootFileSystem, sourcesPath));
                }

                @Override
                public String getFolderName() {
                    return getFileObject().getNameExt();
                }

                @Override
                public boolean isAddSubfoldersSelected() {
                    return true;
                }
            });
            prjParams.setSourceFolders(list.iterator());
        } else {
            addSourceRoot = true;
        }
        prjParams.setSourceFoldersFilter(MakeConfigurationDescriptor.DEFAULT_IGNORE_FOLDERS_PATTERN_EXISTING_PROJECT);
        try {
            lastSelectedProject = ProjectGenerator.getDefault().createProject(prjParams);
            OpenProjects.getDefault().addPropertyChangeListener(this);
            OpenProjects.getDefault().open(new Project[]{lastSelectedProject}, false);
        } catch (IOException ex) {
            Exceptions.printStackTrace(ex);
        }
    }

    @Override
    public void propertyChange(PropertyChangeEvent evt) {
        if (evt.getPropertyName().equals(OpenProjects.PROPERTY_OPEN_PROJECTS)) {
            if (evt.getNewValue() instanceof Project[]) {
                Project[] projects = (Project[])evt.getNewValue();
                if (projects.length == 0) {
                    return;
                }
                OpenProjects.getDefault().removePropertyChangeListener(this);
                if (lastSelectedProject == null) {
                    return;
                }
                RP.post(this::process);
            }
        }
    }

    private void process() {
        ProgressHandle progress = ProgressHandle.createHandle(NbBundle.getMessage(ImportExecutable.class, "ImportExecutable.Progress")); // NOI18N
        progress.start();
        try {
            ConfigurationDescriptorProvider provider = lastSelectedProject.getLookup().lookup(ConfigurationDescriptorProvider.class);
            MakeConfigurationDescriptor configurationDescriptor = provider.getConfigurationDescriptor();
            if (configurationDescriptor == null) {
                return;
            }
            for (Folder folder : configurationDescriptor.getLogicalFolders().getFolders()) {
                if (MakeConfigurationDescriptor.HEADER_FILES_FOLDER.equals(folder.getName()) ||
                    MakeConfigurationDescriptor.RESOURCE_FILES_FOLDER.equals(folder.getName())) {
                    configurationDescriptor.getLogicalFolders().removeFolderAction(folder);
                }
            }
            if (addSourceRoot && sourcesPath != null && sourcesPath.length() > 1) {
                configurationDescriptor.addSourceRoot(sourcesPath);
            }
            configurationDescriptor.setModified();
            configurationDescriptor.save();
        } catch (Throwable ex) {
            Exceptions.printStackTrace(ex);
        } finally {
            progress.finish();
        }
    }
}
