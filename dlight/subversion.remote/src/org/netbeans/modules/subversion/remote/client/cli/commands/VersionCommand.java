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
package org.netbeans.modules.subversion.remote.client.cli.commands;

import java.io.IOException;
import java.util.ArrayList;
import java.util.List;
import java.util.logging.Level;
import org.netbeans.modules.subversion.remote.Subversion;
import org.netbeans.modules.subversion.remote.api.ISVNNotifyListener;
import org.netbeans.modules.subversion.remote.client.cli.SvnCommand;
import org.netbeans.modules.versioning.core.api.VCSFileProxy;
import org.openide.filesystems.FileSystem;

/**
 *
 * 
 */
public class VersionCommand extends SvnCommand {

    public VersionCommand(FileSystem fileSystem) {
        super(fileSystem);
    }

    private final List<String> output = new ArrayList<>();
    private boolean unsupportedVersion = false;
    private boolean supportedMetadataFormat = false;
    static final Version VERSION_15 = Version.parse("1.5"); //NOI18N
    static final Version VERSION_16 = Version.parse("1.6"); //NOI18N
    
    @Override
    protected ISVNNotifyListener.Command getCommand() {
        return ISVNNotifyListener.Command.UNDEFINED;
    }
    
    @Override
    public void prepareCommand(Arguments arguments) throws IOException {
        arguments.add("--version"); //NOI18N
    }

    @Override
    protected void config(VCSFileProxy configDir, String username, String password, Arguments arguments) {
        arguments.addConfigDir(configDir);        
    }

    @Override
    public void outputText(String lineString) {
        if(lineString == null || lineString.trim().equals("")) {
            return;
        }
        output.add(lineString);
        super.outputText(lineString);
    }
    
    public boolean checkForErrors() {
        Integer exitCode = getExitCode();
        if ((exitCode == null) || !exitCode.equals(Integer.valueOf(0))) {
            return false;
        }

        boolean outputProduced = false;

        for (String string : output) {
            if ((string = string.trim()).length() == 0) {     //skip empty lines
                continue;
            }

            outputProduced = true;

            int pos = string.indexOf(" version "); //NOI18N
            if (pos > -1) {
                String vString = string.substring(pos + 9);
                Subversion.LOG.log(Level.INFO, "Commandline client version: {0}", vString); //NOI18N
                // compare numerically: "version 1.14.2" contains "version 1.1"
                Version version = Version.parse(vString);
                if (version.lowerThan(VERSION_15)) {
                    unsupportedVersion = true;
                    return false;
                } else if (version.sameMinor(VERSION_15)
                        || version.sameMinor(VERSION_16)) {
                    supportedMetadataFormat = true;
                }
            }
        }
        return outputProduced;
    }

    public boolean isUnsupportedVersion() {
        return unsupportedVersion;
    }

    public boolean isMetadataFormatSupported() {
        return supportedMetadataFormat;
    }
    
    public String getOutput() {
        StringBuilder sb = new StringBuilder();
        for (String string : output) {
            sb.append(string);
            sb.append('\n');
        }
        return sb.toString();
    }

    /**
     * Version holder with semantic verioning support.
     *
     * Keep svn --version output in mind. The container is compatible with
     * MAJOR.MINOR.PATCH format plus some remainder as it happens in svn
     * --version. I.e. "svn, version 1.10.0 (r1827917)" is supported. Regular
     * 1.10.1 is supported too.
     *
     */
    public static class Version implements Comparable<Version> {

        final int[] parts;
        final String remainder;

        private Version(int[] parts, String remainder) {
            this.parts = parts;
            this.remainder = remainder;
        }

        /**
         * Parse version string into container. String must be in
         * "MAJOR.MINOR.PATCH reminder" format. Reminder part is optional.
         *
         * @param version non null string with version, say "1.10.0 (r1827917)"
         * @return non null version
         * @throws NumberFormatException should parse error occur
         * @throws IllegalArgumentException if one of parameters was null
         */
        public static Version parse(String version) throws NumberFormatException, IllegalArgumentException {
            assertNotNullArg(version, "Version parameter must not be null");
            String[] versionMajorParts = version.split("[^\\d.]", 2);

            String[] stringParts = versionMajorParts[0].split("\\.");
            int[] parts = new int[stringParts.length];
            for (int i = 0; i < stringParts.length; i++) {
                parts[i] = Integer.parseInt(stringParts[i]);
            }

            return new Version(parts, versionMajorParts.length > 1 ? versionMajorParts[1] : "");
        }

        private static void assertNotNullArg(Object value, String errMessage) throws IllegalArgumentException {
            if (value == null) {
                throw new IllegalArgumentException(errMessage);
            }
        }

        @Override
        public int compareTo(Version t) {
            if (t == null) {
                return 1;
            } else {
                for (int i = 0; i < parts.length; i++) {
                    if (t.parts.length <= i) {
                        return 1;
                    }
                    int a = parts[i];
                    int b = t.parts[i];
                    if (a < b) {
                        return -1;
                    } else if (a > b) {
                        return +1;
                    }
                }
                return t.parts.length > parts.length ? -1 : 0;
            }
        }

        public boolean lowerThan(Version that) {
            return compareTo(that) < 0;
        }

        /**
         * Check if MAJOR and MINOR parts are equal
         *
         * @param that can be null
         * @return
         */
        public boolean sameMinor(Version that) {
            return that != null && trim(2).compareTo(that.trim(2)) == 0;
        }

        private Version trim(int level) {
            int[] newParts = new int[level];
            System.arraycopy(parts, 0, newParts, 0, Math.min(level, parts.length));
            return new Version(newParts, this.remainder);
        }

        public boolean greaterThan(Version that) {
            return compareTo(that) > 0;
        }
    }
}
