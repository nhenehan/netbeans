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
package org.netbeans.modules.cnd.makeproject;

import java.io.File;
import java.util.ArrayList;
import java.util.List;
import javax.swing.Action;
import javax.swing.SwingUtilities;
import org.netbeans.api.project.Project;
import org.netbeans.api.project.ProjectManager;
import org.netbeans.api.project.ui.OpenProjects;
import org.netbeans.modules.cnd.api.toolchain.CompilerSet;
import org.netbeans.modules.cnd.api.toolchain.CompilerSetManager;
import org.netbeans.modules.cnd.makeproject.api.MakeProject;
import org.netbeans.modules.cnd.makeproject.api.configurations.ConfigurationDescriptorProvider;
import org.netbeans.modules.cnd.makeproject.api.configurations.MakeConfiguration;
import org.netbeans.modules.cnd.makeproject.api.wizards.ProjectGenerator;
import org.netbeans.modules.cnd.test.CndBaseTestCase;
import org.netbeans.modules.cnd.utils.FSPath;
import org.netbeans.modules.cnd.utils.cache.CndFileUtils;
import org.netbeans.modules.nativeexecution.api.ExecutionEnvironmentFactory;
import org.netbeans.spi.project.support.ProjectOperations;
import org.netbeans.spi.project.ui.CustomizerProvider;
import org.netbeans.spi.project.ui.LogicalViewProvider;
import org.netbeans.spi.project.ui.support.CommonProjectActions;
import org.openide.filesystems.FileObject;
import org.openide.filesystems.FileUtil;
import org.openide.nodes.Node;
import org.openide.util.ContextAwareAction;
import org.openide.util.Utilities;

/**
 * Deleting a project must not break projects created later in the same
 * session: their Properties action must be enabled and their active
 * configuration available.
 */
public class DeleteThenCreateProjectTest extends CndBaseTestCase {

    public DeleteThenCreateProjectTest(String testName) {
        super(testName);
    }

    @Override
    protected boolean addEditorSupport() {
        return false;
    }

    public void testCreateAfterDelete() throws Throwable {
        if (Utilities.isWindows()) {
            return;
        }
        clearWorkDir();
        File workDir = getWorkDir();
        // the Projects tab's root, so the projects' nodes live through the
        // delete and the creations as they do in the IDE
        Class<?> rootClass = Class.forName("org.netbeans.modules.project.ui.ProjectsRootNode"); // NOI18N
        java.lang.reflect.Constructor<?> ctor = rootClass.getDeclaredConstructor(int.class);
        ctor.setAccessible(true);
        final Node[] tab = new Node[1];
        SwingUtilities.invokeAndWait(() -> {
            try {
                tab[0] = (Node) ctor.newInstance(1); // LOGICAL_VIEW
            } catch (ReflectiveOperationException ex) {
                throw new IllegalStateException(ex);
            }
        });
        projectsTab = tab[0];
        MakeProject first = createAndOpen(new File(workDir, "App_1"));
        checkUsable(first);
        delete(first);

        // a new project at another path, and one at the deleted project's path
        MakeProject other = createAndOpen(new File(workDir, "App_2"));
        checkUsable(other);
        MakeProject again = createAndOpen(new File(workDir, "App_1"));
        assertNotSame("the deleted project came back", first, again);
        checkUsable(again);
    }

    private Node projectsTab;

    private MakeProject createAndOpen(File dir) throws Exception {
        FSPath dirF = new FSPath(CndFileUtils.getLocalFileSystem(), dir.getAbsolutePath());
        String hostUID = ExecutionEnvironmentFactory.toUniqueID(ExecutionEnvironmentFactory.getLocal());
        CompilerSet toolchain = CompilerSetManager.get(ExecutionEnvironmentFactory.getLocal()).getDefaultCompilerSet();
        MakeConfiguration debug = MakeConfiguration.createConfiguration(dirF, "Debug", MakeConfiguration.TYPE_APPLICATION, null, hostUID, toolchain, true); // NOI18N
        MakeConfiguration release = MakeConfiguration.createConfiguration(dirF, "Release", MakeConfiguration.TYPE_APPLICATION, null, hostUID, toolchain, true); // NOI18N
        ProjectGenerator.ProjectParameters params = new ProjectGenerator.ProjectParameters(dir.getName(), dir)
                .setConfigurations(new MakeConfiguration[]{debug, release})
                .setMainFile("main|Templates/cppFiles/main.cc") // NOI18N
                .setOpenFlag(false);
        Project created = ProjectGenerator.getDefault().createProject(params);
        assertNotNull("project not created in " + dir, created);
        FileObject dirFO = FileUtil.toFileObject(FileUtil.normalizeFile(dir));
        Project found = ProjectManager.getDefault().findProject(dirFO);
        assertTrue("not a make project: " + found, found instanceof MakeProject);
        MakeProject project = (MakeProject) found;
        assertFalse("found a deleted project at " + dir, ((MakeProjectImpl) project).isDeleted());
        OpenProjects.getDefault().open(new Project[]{project}, false);
        OpenProjects.getDefault().openProjects().get();
        return project;
    }

    private void checkUsable(MakeProject project) throws Exception {
        String name = project.getProjectDirectory().getNameExt();
        ConfigurationDescriptorProvider cdp = project.getLookup().lookup(ConfigurationDescriptorProvider.class);
        assertNotNull(name + ": no ConfigurationDescriptorProvider", cdp);
        assertNotNull(name + ": no descriptor", cdp.getConfigurationDescriptor());
        waitFor(() -> project.getActiveConfiguration() != null);
        assertNotNull(name + ": no active configuration", project.getActiveConfiguration());
        assertNotNull(name + ": no CustomizerProvider", project.getLookup().lookup(CustomizerProvider.class));

        LogicalViewProvider lvp = project.getLookup().lookup(LogicalViewProvider.class);
        assertNotNull(name + ": no LogicalViewProvider", lvp);
        final Node[] root = new Node[1];
        SwingUtilities.invokeAndWait(() -> root[0] = lvp.createLogicalView());
        List<Project> inContext = new ArrayList<>(root[0].getLookup().lookupAll(Project.class));
        assertEquals(name + ": projects in the node's context " + inContext, 1, inContext.size());
        assertSame(name + ": the node holds another project", project, inContext.get(0));

        assertTrue(name + ": Properties is disabled", isCustomizeEnabled(root[0]));

        // the same in the Projects tab, where projectui wraps the node
        Node tabNode = null;
        for (int i = 0; i < 50 && tabNode == null; i++) {
            for (Node n : projectsTab.getChildren().getNodes(true)) {
                Project p = n.getLookup().lookup(Project.class);
                if (p != null && p.getProjectDirectory().equals(project.getProjectDirectory())) {
                    tabNode = n;
                }
            }
            if (tabNode == null) {
                Thread.sleep(100);
            }
        }
        assertNotNull(name + ": not in the Projects tab", tabNode);
        final Node shown = tabNode;
        waitFor(() -> shown.getLookup().lookup(Project.class) == project);
        List<Project> inTab = new ArrayList<>(tabNode.getLookup().lookupAll(Project.class));
        assertEquals(name + ": projects in the Projects tab node " + inTab, 1, inTab.size());
        assertSame(name + ": the Projects tab node holds another project", project, inTab.get(0));
        assertTrue(name + ": Properties is disabled in the Projects tab", isCustomizeEnabled(tabNode));
    }

    private static boolean isCustomizeEnabled(Node node) throws Exception {
        Action customize = CommonProjectActions.customizeProjectAction();
        Action inNode = ((ContextAwareAction) customize).createContextAwareInstance(node.getLookup());
        final boolean[] enabled = new boolean[1];
        SwingUtilities.invokeAndWait(() -> enabled[0] = inNode.isEnabled());
        return enabled[0];
    }

    private void delete(MakeProject project) throws Exception {
        OpenProjects.getDefault().close(new Project[]{project});
        List<FileObject> toDelete = new ArrayList<>();
        toDelete.addAll(ProjectOperations.getMetadataFiles(project));
        toDelete.addAll(ProjectOperations.getDataFiles(project));
        ProjectOperations.notifyDeleting(project);
        for (FileObject f : toDelete) {
            if (f != null && f.isValid()) {
                f.delete();
            }
        }
        FileObject dir = project.getProjectDirectory();
        dir.refresh();
        if (dir.isValid() && dir.getChildren().length == 0) {
            dir.delete();
        }
        ProjectOperations.notifyDeleted(project);
        // closing saves the project asynchronously; that save must not
        // write nbproject/ back into the deleted directory
        File d = FileUtil.toFile(dir);
        Thread.sleep(2000);
        assertFalse("files left after deleting " + d + ": " + java.util.Arrays.toString(d.list())
                + ", nbproject: " + java.util.Arrays.toString(new File(d, "nbproject").list()), d.exists());
    }

    private static void waitFor(java.util.function.BooleanSupplier condition) throws InterruptedException {
        for (int i = 0; i < 100 && !condition.getAsBoolean(); i++) {
            Thread.sleep(100);
        }
    }
}
