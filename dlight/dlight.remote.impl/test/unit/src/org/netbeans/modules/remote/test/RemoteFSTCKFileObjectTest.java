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
package org.netbeans.modules.remote.test;

import java.io.IOException;
import junit.framework.Test;
import org.netbeans.junit.NbTestSuite;
import org.netbeans.modules.remote.impl.fs.RemoteFSTCKTestCase;
import org.openide.filesystems.FileObjectTestHid;

/**
 *
 */
public class RemoteFSTCKFileObjectTest extends RemoteFSTCKTestCase {
   
    public RemoteFSTCKFileObjectTest(Test test) {
        super(test);
    }
    
    public static Test suite() {
        NbTestSuite suite = new NbTestSuite();
        suite.addTestSuite(FileObjectTestHidEx.class);
        return new RemoteFSTCKFileObjectTest(suite);
    }    

    public static class FileObjectTestHidEx extends FileObjectTestHid {

        public FileObjectTestHidEx(String name) {
            super(name);
        }

        @Override
        protected void setUp() throws Exception {
            RemoteTestSuiteBase.registerTestSetup(this);
            super.setUp();
        }        
        
        @Override
        protected void tearDown() throws Exception {
            super.tearDown();
            RemoteTestSuiteBase.registerTestTearDown(this);
        }

        // These two count file accesses through a SecurityManager
        // (org.openide.filesystems.test.StatFiles). Since JDK 18 that needs
        // -Djava.security.manager=allow, and JDK 24+ refuses to start with it
        // (JEP 486), so they only run where it is allowed explicitly.

        @Override
        public void testGetMIMETypeCached() throws IOException {
            if (securityManagerSupported()) {
                super.testGetMIMETypeCached();
            }
        }

        @Override
        public void testGetMIMETypeCachedInAtomicAction() throws IOException {
            if (securityManagerSupported()) {
                super.testGetMIMETypeCachedInAtomicAction();
            }
        }

        private static boolean securityManagerSupported() {
            return Runtime.version().feature() < 24
                    && "allow".equals(System.getProperty("java.security.manager")); // NOI18N
        }
    }     
}
