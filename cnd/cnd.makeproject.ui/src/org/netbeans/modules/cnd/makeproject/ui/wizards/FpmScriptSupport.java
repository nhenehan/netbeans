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

import java.io.File;
import java.io.IOException;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import javax.swing.filechooser.FileFilter;
import org.netbeans.modules.cnd.api.toolchain.CompilerSet;
import org.netbeans.modules.cnd.makeproject.api.wizards.BuildSupport.BuildFile;
import org.netbeans.modules.cnd.makeproject.api.wizards.BuildSupport.BuildFileProvider;
import org.netbeans.modules.cnd.utils.CndPathUtilities;
import org.netbeans.modules.nativeexecution.api.ExecutionEnvironment;
import org.openide.filesystems.FileObject;
import org.openide.util.NbBundle;
import org.openide.util.lookup.ServiceProvider;

/**
 * Builds Fortran Package Manager (fpm) projects: a folder with an
 * {@code fpm.toml} is imported like a makefile project whose build and clean
 * commands run fpm.
 * <p>
 * The build uses the debug profile, because a plain {@code fpm build} compiles
 * without {@code -g}, and installs into {@code dist/} under the project, because
 * fpm's own output folder name ({@code build/gfortran_<hash>}) changes with the
 * compiler flags. So the executable has a stable path for Run and Debug:
 * {@code dist/bin/<name>}.
 * <p>
 * Positioned after the makefile provider: a folder that also has a Makefile
 * keeps building with make.
 */
@ServiceProvider(service = BuildFileProvider.class, position = 6000)
public class FpmScriptSupport implements BuildFileProvider {

    static final String FPM_TOML = "fpm.toml"; // NOI18N
    static final String INSTALL_PREFIX = "dist"; // NOI18N
    static final String BUILD_COMMAND = "fpm install --profile debug --prefix " + INSTALL_PREFIX; // NOI18N
    static final String CLEAN_COMMAND = "fpm clean --all"; // NOI18N

    private static final Pattern TABLE = Pattern.compile("^\\s*\\[\\[?\\s*([A-Za-z0-9_.-]+)\\s*\\]\\]?"); // NOI18N
    private static final Pattern NAME = Pattern.compile("^\\s*name\\s*=\\s*[\"']([^\"']+)[\"']"); // NOI18N

    @Override
    public BuildFile findBuildFileInFolder(FileObject folder, ExecutionEnvironment ee, CompilerSet compilerSet) {
        if (folder == null || !(folder.isFolder() && folder.canRead())) {
            return null;
        }
        FileObject toml = folder.getFileObject(FPM_TOML);
        if (toml != null && toml.isValid() && toml.isData() && toml.canRead()) {
            return new FpmScriptArtifact(toml.getPath());
        }
        return null;
    }

    @Override
    public BuildFile scriptToBuildFile(String script) {
        if (FPM_TOML.equals(CndPathUtilities.getBaseName(script))) {
            return new FpmScriptArtifact(script);
        }
        return null;
    }

    @Override
    public boolean isSupported(BuildFile script) {
        return script instanceof FpmScriptArtifact;
    }

    @Override
    public FileFilter[] getFileFilter() {
        return new FileFilter[]{new FileFilter() {
            @Override
            public boolean accept(File f) {
                return f.isDirectory() || FPM_TOML.equals(f.getName());
            }

            @Override
            public String getDescription() {
                return NbBundle.getMessage(FpmScriptSupport.class, "FILECHOOSER_FPM_FILEFILTER"); // NOI18N
            }
        }};
    }

    @Override
    public String getHint() {
        return NbBundle.getMessage(SelectModePanel.class, "SelectModeSimpleInstructionExtraText_Fpm"); // NOI18N
    }

    static boolean isFpm(BuildFile buildFile) {
        return buildFile instanceof FpmScriptArtifact;
    }

    /**
     * The absolute path of the executable that {@link #BUILD_COMMAND} installs:
     * {@code <project>/dist/bin/<name>}, where the name is the first
     * {@code [[executable]]} of the manifest, or else the package name if the
     * package has an {@code app/} folder (fpm's default executable).
     *
     * @param fpmToml The manifest.
     * @return The executable path, or null for a library-only package.
     */
    static String getExecutable(FileObject fpmToml) {
        if (fpmToml == null || !fpmToml.isValid()) {
            return null;
        }
        String text;
        try {
            text = fpmToml.asText();
        } catch (IOException ex) {
            return null;
        }
        String name = executableName(text, fpmToml.getParent().getFileObject("app") != null); // NOI18N
        if (name == null) {
            return null;
        }
        return fpmToml.getParent().getPath() + '/' + INSTALL_PREFIX + "/bin/" + name; // NOI18N
    }

    /**
     * Reads the executable name from an fpm manifest. Only the few keys needed
     * here are looked at, not the whole TOML syntax.
     */
    static String executableName(String manifest, boolean hasAppFolder) {
        String packageName = null;
        String table = ""; // top level
        for (String line : manifest.split("\\R")) { // NOI18N
            Matcher t = TABLE.matcher(line);
            if (t.find()) {
                table = t.group(1);
                continue;
            }
            Matcher n = NAME.matcher(line);
            if (n.find()) {
                if (table.isEmpty() && packageName == null) {
                    packageName = n.group(1);
                } else if ("executable".equals(table)) { // NOI18N
                    return n.group(1);
                }
            }
        }
        return hasAppFolder ? packageName : null;
    }

    private static final class FpmScriptArtifact implements BuildFile {

        private final String script;

        FpmScriptArtifact(String script) {
            this.script = script;
        }

        @Override
        public String getFile() {
            return script;
        }

        @Override
        public String getCleanCommandLine(String arguments, String workingDir) {
            return withArguments(CLEAN_COMMAND, arguments);
        }

        @Override
        public String getBuildCommandLine(String arguments, String workingDir) {
            return withArguments(BUILD_COMMAND, arguments);
        }

        private static String withArguments(String command, String arguments) {
            return arguments == null || arguments.isEmpty() ? command : command + ' ' + arguments;
        }

        @Override
        public String validate(ExecutionEnvironment ee, CompilerSet compilerSet) {
            return null;
        }
    }
}
