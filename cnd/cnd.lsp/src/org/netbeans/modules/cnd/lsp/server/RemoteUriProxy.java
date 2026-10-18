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

import java.io.BufferedInputStream;
import java.io.ByteArrayOutputStream;
import java.io.EOFException;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.util.Locale;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * Rewrites document URIs in the LSP messages exchanged with a clangd that
 * runs on a remote host.
 * <p>
 * The IDE names remote files with remote file system URIs
 * ({@code rfs://user@host:port/path}), clangd only understands {@code file:}
 * URIs of its own host. Outgoing messages get {@code rfs://user@host:port/}
 * replaced by {@code file:///}, incoming messages get the reverse, so system
 * headers that clangd points to (e.g. {@code /usr/include/stdio.h}) open from
 * the remote file system too. The path part is left untouched: both sides use
 * URL escaping.
 * <p>
 * Only JSON string values that start with the prefix are rewritten. A quote
 * that opens a JSON string is never preceded by a backslash, so document text
 * that merely contains such a URI (inside an escaped string) is left alone.
 */
final class RemoteUriProxy {

    private static final String FILE_PREFIX = "file:///"; // NOI18N
    private static final String CONTENT_LENGTH = "content-length:"; // NOI18N

    private final Pattern toServer;
    private final String toServerReplacement;
    private final Pattern toClient;
    private final String toClientReplacement;

    /**
     * @param remotePrefix the remote file system URI of the host root, without
     * the trailing slash, e.g. {@code rfs://user@host:22}
     */
    RemoteUriProxy(String remotePrefix) {
        String prefix = remotePrefix.endsWith("/") ? remotePrefix : remotePrefix + '/'; // NOI18N
        toServer = Pattern.compile("(?<!\\\\)\"" + Pattern.quote(prefix)); // NOI18N
        toServerReplacement = Matcher.quoteReplacement('"' + FILE_PREFIX);
        toClient = Pattern.compile("(?<!\\\\)\"" + Pattern.quote(FILE_PREFIX)); // NOI18N
        toClientReplacement = Matcher.quoteReplacement('"' + prefix);
    }

    String toServer(String json) {
        return toServer.matcher(json).replaceAll(toServerReplacement);
    }

    String toClient(String json) {
        return toClient.matcher(json).replaceAll(toClientReplacement);
    }

    /**
     * Wraps the stream the IDE writes requests to (clangd's stdin).
     */
    OutputStream wrapToServer(OutputStream out) {
        return new RewritingOutputStream(out);
    }

    /**
     * Wraps the stream the IDE reads responses from (clangd's stdout).
     */
    InputStream wrapToClient(InputStream in) {
        return new RewritingInputStream(in);
    }

    /**
     * Encodes a message body with a Content-Length header.
     */
    static byte[] frame(String json) {
        byte[] body = json.getBytes(StandardCharsets.UTF_8);
        byte[] header = ("Content-Length: " + body.length + "\r\n\r\n").getBytes(StandardCharsets.US_ASCII); // NOI18N
        byte[] message = new byte[header.length + body.length];
        System.arraycopy(header, 0, message, 0, header.length);
        System.arraycopy(body, 0, message, header.length, body.length);
        return message;
    }

    /**
     * Parses the Content-Length out of a header block (ASCII, lines ended by
     * CRLF), or returns -1 if there is none.
     */
    static int contentLength(String headers) {
        for (String line : headers.split("\r\n")) { // NOI18N
            if (line.toLowerCase(Locale.ROOT).startsWith(CONTENT_LENGTH)) {
                return Integer.parseInt(line.substring(CONTENT_LENGTH.length()).trim());
            }
        }
        return -1;
    }

    /**
     * Collects bytes until a full message (headers and body) is available,
     * then writes the rewritten message downstream. lsp4j writes a message
     * with several write calls and flushes after each message.
     */
    private final class RewritingOutputStream extends OutputStream {

        private final OutputStream out;
        private final ByteArrayOutputStream pending = new ByteArrayOutputStream();

        RewritingOutputStream(OutputStream out) {
            this.out = out;
        }

        @Override
        public synchronized void write(int b) throws IOException {
            pending.write(b);
            drain();
        }

        @Override
        public synchronized void write(byte[] b, int off, int len) throws IOException {
            pending.write(b, off, len);
            drain();
        }

        private void drain() throws IOException {
            while (true) {
                byte[] buffer = pending.toByteArray();
                int headerEnd = indexOfHeaderEnd(buffer, buffer.length);
                if (headerEnd < 0) {
                    return;
                }
                String headers = new String(buffer, 0, headerEnd, StandardCharsets.US_ASCII);
                int length = contentLength(headers);
                if (length < 0) {
                    throw new IOException("LSP message without Content-Length: " + headers); // NOI18N
                }
                int bodyStart = headerEnd + 4;
                if (buffer.length < bodyStart + length) {
                    return;
                }
                String body = new String(buffer, bodyStart, length, StandardCharsets.UTF_8);
                out.write(frame(toServer(body)));
                pending.reset();
                pending.write(buffer, bodyStart + length, buffer.length - bodyStart - length);
            }
        }

        @Override
        public synchronized void flush() throws IOException {
            out.flush();
        }

        @Override
        public synchronized void close() throws IOException {
            out.close();
        }
    }

    /**
     * Reads one whole message at a time from clangd and serves the rewritten
     * message.
     */
    private final class RewritingInputStream extends InputStream {

        private final InputStream in;
        private byte[] current = new byte[0];
        private int position = 0;

        RewritingInputStream(InputStream in) {
            // headers are read byte by byte
            this.in = new BufferedInputStream(in);
        }

        private boolean fill() throws IOException {
            if (position < current.length) {
                return true;
            }
            ByteArrayOutputStream headers = new ByteArrayOutputStream();
            int b;
            while ((b = in.read()) >= 0) {
                headers.write(b);
                byte[] h = headers.toByteArray();
                if (indexOfHeaderEnd(h, h.length) >= 0) {
                    break;
                }
            }
            if (b < 0) {
                return false;
            }
            String headerText = headers.toString(StandardCharsets.US_ASCII.name());
            int length = contentLength(headerText);
            if (length < 0) {
                throw new IOException("LSP message without Content-Length: " + headerText); // NOI18N
            }
            byte[] body = new byte[length];
            int read = 0;
            while (read < length) {
                int n = in.read(body, read, length - read);
                if (n < 0) {
                    throw new EOFException("clangd closed the stream inside a message"); // NOI18N
                }
                read += n;
            }
            current = frame(toClient(new String(body, StandardCharsets.UTF_8)));
            position = 0;
            return true;
        }

        @Override
        public int read() throws IOException {
            if (!fill()) {
                return -1;
            }
            return current[position++] & 0xff;
        }

        @Override
        public int read(byte[] b, int off, int len) throws IOException {
            if (len == 0) {
                return 0;
            }
            if (!fill()) {
                return -1;
            }
            int n = Math.min(len, current.length - position);
            System.arraycopy(current, position, b, off, n);
            position += n;
            return n;
        }

        @Override
        public int available() {
            return current.length - position;
        }

        @Override
        public void close() throws IOException {
            in.close();
        }
    }

    /**
     * Returns the index of the CRLFCRLF that ends the headers, or -1.
     */
    private static int indexOfHeaderEnd(byte[] b, int length) {
        for (int i = 0; i + 3 < length; i++) {
            if (b[i] == '\r' && b[i + 1] == '\n' && b[i + 2] == '\r' && b[i + 3] == '\n') {
                return i;
            }
        }
        return -1;
    }
}
