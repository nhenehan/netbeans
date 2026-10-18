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
package org.netbeans.modules.cnd.lsp.server;

import java.io.BufferedReader;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.io.OutputStream;
import java.lang.ProcessBuilder.Redirect;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.logging.Level;
import java.util.logging.Logger;
import javax.swing.event.SwingPropertyChangeSupport;
import org.netbeans.api.project.Project;
import org.netbeans.modules.lsp.client.spi.LanguageServerProvider.LanguageServerDescription;
import org.netbeans.modules.lsp.client.spi.ServerRestarter;
import org.netbeans.modules.nativeexecution.api.ExecutionEnvironment;
import org.netbeans.modules.nativeexecution.api.ExecutionEnvironmentFactory;
import org.netbeans.modules.nativeexecution.api.NativeProcess;
import org.netbeans.modules.nativeexecution.api.NativeProcessBuilder;
import org.netbeans.modules.nativeexecution.api.util.ConnectionManager;
import org.netbeans.modules.nativeexecution.api.util.HostInfoUtils;
import org.netbeans.modules.remote.spi.FileSystemProvider;
import org.openide.awt.NotificationDisplayer;
import org.openide.filesystems.FileObject;
import org.openide.util.Exceptions;
import org.openide.util.ImageUtilities;
import org.openide.util.NbBundle;
import org.openide.util.RequestProcessor;

/**
 * The clangd LSP server of one host, serving all projects whose files live on
 * that host. Local projects use a local clangd; full remote projects use a
 * clangd started on the remote host, behind a {@link RemoteUriProxy}.
 *
 * @author antonio
 */
public final class ClangdProcess {

    private static final Logger LOG = Logger.getLogger(ClangdProcess.class.getName());

    private static final Map<ExecutionEnvironment, ClangdProcess> PROCESSES = new HashMap<>();

    // where to look for clangd on remote hosts, besides the user's PATH
    private static final List<String> REMOTE_SEARCH_PATHS = Arrays.asList(
            "/usr/bin", "/usr/local/bin", "/opt/homebrew/bin"); // NOI18N

    private static final RequestProcessor STDERR_RP = new RequestProcessor("clangd stderr", 8); // NOI18N

    /**
     * Returns the ClangdProcess of the local host.
     *
     * @return The ClangdProcess of the local host.
     */
    public static final ClangdProcess getInstance() {
        return forEnvironment(ExecutionEnvironmentFactory.getLocal());
    }

    /**
     * Returns the ClangdProcess of the host the project's files live on.
     *
     * @param project The project.
     * @return The ClangdProcess for the project.
     */
    public static ClangdProcess forProject(Project project) {
        return forEnvironment(FileSystemProvider.getExecutionEnvironment(project.getProjectDirectory()));
    }

    /**
     * Returns the ClangdProcess of a host.
     *
     * @param env The host.
     * @return The ClangdProcess for the host.
     */
    public static ClangdProcess forEnvironment(ExecutionEnvironment env) {
        synchronized (PROCESSES) {
            return PROCESSES.computeIfAbsent(env, ClangdProcess::new);
        }
    }

    /**
     * Restarts every clangd that is running, e.g. after the options changed.
     */
    public static void restartAll() {
        List<ClangdProcess> processes;
        synchronized (PROCESSES) {
            processes = new ArrayList<>(PROCESSES.values());
        }
        for (ClangdProcess clangd : processes) {
            if (clangd.process != null) {
                clangd.restart();
            }
        }
    }

    public static final String PROP_STATE = "state"; // NOI18N

    private final class ClangdProcessWatcher extends Thread {

        private final Process process;

        public ClangdProcessWatcher(Process process) {
            super("clangd process watcher " + env.getDisplayName()); // NOI18N
            this.process = process;
        }

        @Override
        public void run() {
            try {
                if (process.isAlive()) {
                    int exitCode = process.waitFor();
                    LOG.log(Level.INFO, "clangd process on {0} exited with code {1}", new Object[]{env.getDisplayName(), exitCode});
                    // after a restart the old process exits while the new one
                    // runs: only the current process may report its end
                    if (process == ClangdProcess.this.process) {
                        fireStateChange(LSPServerState.STOPPED);
                    }
                }
            } catch (InterruptedException e) {
                LOG.log(Level.INFO, "clangd process exited with error {0}:{1}", new Object[] { e.getMessage(), e.getClass().getName() });
                fireStateChange(LSPServerState.ERROR);
            }
        }
    }

    private final ExecutionEnvironment env;
    private final SwingPropertyChangeSupport propertyChangeSupport;
    private Process process;
    private String remoteClangd;
    private boolean missingClangdNotified;
    private LanguageServerDescription languageServerDescription;
    private LSPServerState serverState;
    private ServerRestarter serverRestarter;
    private ClangdProcessWatcher processWatcher;

    private ClangdProcess(ExecutionEnvironment env) {
        this.env = env;
        this.propertyChangeSupport = new SwingPropertyChangeSupport(this, true);
        this.serverState = LSPServerState.STOPPED;
    }

    /**
     * Returns the state of clangd.
     *
     * @return The state of the clangd process.
     */
    public LSPServerState getState() {
        return serverState;
    }

    public ExecutionEnvironment getExecutionEnvironment() {
        return env;
    }

    public String[] getCommands() {
        ClangdOptions options = ClangdOptions.load();
        ArrayList<String> commands = new ArrayList<>();
        if (env.isLocal()) {
            File clangd = options.getClangdExecutable();
            commands.add(clangd == null ? "clangd" : clangd.getAbsolutePath()); // NOI18N
        } else {
            commands.add(remoteClangd == null ? "clangd" : remoteClangd); // NOI18N
        }
        commands.add("--clang-tidy"); // NOI18N
        commands.add("--completion-style=detailed"); // NOI18N
        commands.add("--offset-encoding=utf-8"); // NOI18N
        commands.addAll(options.getCommandLineOptions());
        return commands.toArray(new String[commands.size()]);
    }

    public void start(ServerRestarter serverRestarter) throws IOException {
        this.serverRestarter = serverRestarter;
        if (this.process != null && this.process.isAlive()) {
            fireStateChange(LSPServerState.STOPPED);
            if (serverRestarter != null) {
                serverRestarter.restart();
            }
            // The restarter only shuts down the bindings of the project it
            // was created for, so make sure the old clangd does not linger
            if (this.process.isAlive()) {
                this.process.destroy();
            }
        }
        if (env.isRemote()) {
            startRemote();
            return;
        }
        ClangdOptions options = ClangdOptions.load();
        if (options.isMisconfigured()) {
            fireStateChange(LSPServerState.MISCONFIGURED);
            File clangd = options.getLogFile();
            throw new IOException(String.format("Cannot execute clangd at path: %s", clangd == null ? "[NOT SET]" : clangd.getAbsolutePath()));
        } else {
            fireStateChange(LSPServerState.STOPPED);
        }
        String[] commands = getCommands();
        try {
            ProcessBuilder processBuilder = new ProcessBuilder(commands);
            File logFile = options.getLogFile();
            if (logFile != null) {
                processBuilder.redirectError(logFile);
            } else {
                processBuilder.redirectError(Redirect.INHERIT);
            }
            this.process = processBuilder.start();
            LOG.log(Level.INFO, String.format("Started clangd with arguments: %s",
                    String.join(" ", commands)));
            this.languageServerDescription = LanguageServerDescription.create(process.getInputStream(), process.getOutputStream(), process);
            fireStateChange(LSPServerState.RUNNING);
            this.processWatcher = new ClangdProcessWatcher(process);
            this.processWatcher.start();
        } catch (Throwable e) {
            LOG.log(Level.SEVERE, String.format("Failed to start LSP Server: %s:%s",
                    e.getMessage(), e.getClass().getName()), e);
            fireStateChange(LSPServerState.ERROR);
            throw e;
        }
    }

    @NbBundle.Messages({
        "# {0} - host",
        "ClangdProcess.missing.title=clangd not found on {0}",
        "# {0} - host",
        "ClangdProcess.missing.details=Install clangd on {0} for code assistance in remote projects."
    })
    private void startRemote() throws IOException {
        if (!ConnectionManager.getInstance().isConnectedTo(env)) {
            fireStateChange(LSPServerState.STOPPED);
            throw new IOException("Not connected to " + env.getDisplayName()); // NOI18N
        }
        if (remoteClangd == null) {
            remoteClangd = HostInfoUtils.searchFile(env, REMOTE_SEARCH_PATHS, "clangd", true); // NOI18N
        }
        if (remoteClangd == null) {
            fireStateChange(LSPServerState.MISCONFIGURED);
            if (!missingClangdNotified) {
                missingClangdNotified = true;
                NotificationDisplayer.getDefault().notify(
                        Bundle.ClangdProcess_missing_title(env.getDisplayName()),
                        ImageUtilities.loadImageIcon("org/netbeans/modules/cnd/lsp/makeproject/ui/resources/clangd.llvm.org.png", false), // NOI18N
                        Bundle.ClangdProcess_missing_details(env.getDisplayName()),
                        null);
            }
            throw new IOException("clangd not found on " + env.getDisplayName()); // NOI18N
        }
        fireStateChange(LSPServerState.STOPPED);
        String[] commands = getCommands();
        try {
            NativeProcess remote = NativeProcessBuilder.newProcessBuilder(env)
                    .setExecutable(commands[0])
                    .setArguments(Arrays.copyOfRange(commands, 1, commands.length))
                    .setUsePty(false)
                    .call();
            this.process = remote;
            LOG.log(Level.INFO, String.format("Started clangd on %s with arguments: %s",
                    env.getDisplayName(), String.join(" ", commands)));
            drainErrorStream(remote.getErrorStream(), ClangdOptions.load().getLogFile());
            RemoteUriProxy proxy = new RemoteUriProxy(remotePrefix());
            this.languageServerDescription = LanguageServerDescription.create(
                    proxy.wrapToClient(remote.getInputStream()),
                    proxy.wrapToServer(remote.getOutputStream()),
                    remote);
            fireStateChange(LSPServerState.RUNNING);
            this.processWatcher = new ClangdProcessWatcher(remote);
            this.processWatcher.start();
        } catch (Throwable e) {
            LOG.log(Level.SEVERE, String.format("Failed to start LSP Server on %s: %s:%s",
                    env.getDisplayName(), e.getMessage(), e.getClass().getName()), e);
            fireStateChange(LSPServerState.ERROR);
            throw e;
        }
    }

    /**
     * The remote file system URI of the host root, as the IDE sends it, e.g.
     * {@code rfs://user@host:22/}.
     */
    private String remotePrefix() throws IOException {
        FileObject root = FileSystemProvider.getFileObject(env, "/"); // NOI18N
        if (root == null) {
            throw new IOException("No remote file system for " + env.getDisplayName()); // NOI18N
        }
        return root.toURI().toString();
    }

    /**
     * clangd logs to stderr, which has to be consumed or clangd blocks once
     * the remote channel's buffer is full.
     */
    private void drainErrorStream(InputStream err, File logFile) {
        STDERR_RP.post(() -> {
            try (OutputStream log = logFile == null ? null : new FileOutputStream(logFile, true);
                    BufferedReader reader = new BufferedReader(new InputStreamReader(err, StandardCharsets.UTF_8))) {
                String line;
                while ((line = reader.readLine()) != null) {
                    if (log != null) {
                        log.write((line + '\n').getBytes(StandardCharsets.UTF_8));
                    } else if (LOG.isLoggable(Level.FINEST)) {
                        LOG.log(Level.FINEST, "[{0}] {1}", new Object[]{env.getDisplayName(), line});
                    }
                }
            } catch (IOException ex) {
                LOG.log(Level.FINE, "clangd stderr closed", ex);
            }
        });
    }

    public LanguageServerDescription getLanguageServerDescription() {
        return languageServerDescription;
    }

    /**
     * Only LSPServerSupport is expected to be a listener of this class. Other
     * objects should listen to LSPServerSupport in the project's lookup.
     *
     * @param propertyChangeListener
     */
    void addPropertyChangeListener(LSPServerSupport propertyChangeListener) {
        propertyChangeSupport.addPropertyChangeListener(propertyChangeListener);
    }

    /**
     * Only LSPServerSupport is expected to be a listener of this class. Other
     * objects should listen to LSPServerSupport in the project's lookup.
     *
     * @param propertyChangeListener
     */
    void removePropertyChangeListener(LSPServerSupport propertyChangeListener) {
        propertyChangeSupport.removePropertyChangeListener(propertyChangeListener);
    }

    private void fireStateChange(LSPServerState newState) {
        LSPServerState oldState = this.serverState;
        if (oldState != newState) {
            this.serverState = newState;
            LOG.log(Level.INFO, "Clangd server state changed from {0} to {1}", new Object[] { oldState, newState});
            propertyChangeSupport.firePropertyChange(PROP_STATE, oldState, newState);
        }
    }

    public void restart() {
        try {
            start(this.serverRestarter);
        } catch (IOException ex) {
            Exceptions.printStackTrace(ex);
        }
    }

}
