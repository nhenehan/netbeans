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

import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import org.junit.Test;
import static org.junit.Assert.*;

public class RemoteUriProxyTest {

    private static final String PREFIX = "rfs://nbtest@127.0.0.1:2222/"; // NOI18N

    private final RemoteUriProxy proxy = new RemoteUriProxy(PREFIX);

    @Test
    public void testRewritesUrisBothWays() {
        String toClangd = "{\"textDocument\":{\"uri\":\"rfs://nbtest@127.0.0.1:2222/home/nbtest/debugdemo/main.c\"},"
                + "\"rootUri\":\"rfs://nbtest@127.0.0.1:2222/home/nbtest/debugdemo/\"}";
        String expected = "{\"textDocument\":{\"uri\":\"file:///home/nbtest/debugdemo/main.c\"},"
                + "\"rootUri\":\"file:///home/nbtest/debugdemo/\"}";
        assertEquals(expected, proxy.toServer(toClangd));
        assertEquals(toClangd, proxy.toClient(expected));
    }

    @Test
    public void testSystemHeadersMapToRemoteFileSystem() {
        String location = "[{\"uri\":\"file:///usr/include/stdio.h\",\"range\":{}}]";
        assertEquals("[{\"uri\":\"rfs://nbtest@127.0.0.1:2222/usr/include/stdio.h\",\"range\":{}}]",
                proxy.toClient(location));
    }

    @Test
    public void testWorkspaceEditKeysAreRewritten() {
        String edit = "{\"changes\":{\"file:///home/nbtest/a.c\":[]}}";
        assertEquals("{\"changes\":{\"rfs://nbtest@127.0.0.1:2222/home/nbtest/a.c\":[]}}",
                proxy.toClient(edit));
    }

    @Test
    public void testDocumentTextIsNotRewritten() {
        // a C string literal holding a URI is an escaped quote inside the JSON text
        String didOpen = "{\"text\":\"const char *u = \\\"file:///etc/passwd\\\";\","
                + "\"uri\":\"rfs://nbtest@127.0.0.1:2222/home/nbtest/x.c\"}";
        String rewritten = proxy.toServer(didOpen);
        assertTrue(rewritten.contains("\\\"file:///etc/passwd\\\""));
        assertTrue(rewritten.contains("\"uri\":\"file:///home/nbtest/x.c\""));
        String back = proxy.toClient(rewritten);
        assertEquals(didOpen, back);
    }

    @Test
    public void testOtherHostsAreLeftAlone() {
        String other = "{\"uri\":\"rfs://someone@elsewhere:22/home/x.c\"}";
        assertEquals(other, proxy.toServer(other));
    }

    @Test
    public void testOutputStreamReframesSplitWrites() throws Exception {
        String body = "{\"uri\":\"rfs://nbtest@127.0.0.1:2222/home/nbtest/été.c\"}";
        byte[] message = RemoteUriProxy.frame(body);
        ByteArrayOutputStream sink = new ByteArrayOutputStream();
        OutputStream out = proxy.wrapToServer(sink);
        // header and body in separate writes, the body split mid-character
        int split = message.length - 5;
        out.write(message, 0, 10);
        out.write(message, 10, split - 10);
        assertEquals(0, sink.size());
        out.write(message, split, message.length - split);
        out.flush();
        String expectedBody = "{\"uri\":\"file:///home/nbtest/été.c\"}";
        assertArrayEquals(RemoteUriProxy.frame(expectedBody), sink.toByteArray());
    }

    @Test
    public void testInputStreamReframesMessages() throws Exception {
        String first = "{\"id\":1,\"result\":{\"uri\":\"file:///usr/include/stdio.h\"}}";
        String second = "{\"id\":2,\"result\":null}";
        ByteArrayOutputStream raw = new ByteArrayOutputStream();
        raw.write(RemoteUriProxy.frame(first));
        raw.write("Content-Type: application/vscode-jsonrpc; charset=utf-8\r\n".getBytes(StandardCharsets.US_ASCII));
        raw.write(RemoteUriProxy.frame(second));
        InputStream in = proxy.wrapToClient(new ByteArrayInputStream(raw.toByteArray()));
        ByteArrayOutputStream read = new ByteArrayOutputStream();
        byte[] buffer = new byte[7];
        int n;
        while ((n = in.read(buffer)) > 0) {
            read.write(buffer, 0, n);
        }
        ByteArrayOutputStream expected = new ByteArrayOutputStream();
        expected.write(RemoteUriProxy.frame("{\"id\":1,\"result\":{\"uri\":\"rfs://nbtest@127.0.0.1:2222/usr/include/stdio.h\"}}"));
        expected.write(RemoteUriProxy.frame(second));
        assertArrayEquals(expected.toByteArray(), read.toByteArray());
    }

    @Test
    public void testContentLengthIsCaseInsensitive() {
        assertEquals(42, RemoteUriProxy.contentLength("content-length: 42\r\nContent-Type: x"));
        assertEquals(-1, RemoteUriProxy.contentLength("Content-Type: x"));
    }
}
