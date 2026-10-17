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
package org.netbeans.modules.cnd.utils.cache;

import java.lang.ref.Reference;
import java.lang.ref.WeakReference;
import org.netbeans.junit.NbTestCase;

/**
 * Interning contract of {@link APTStringManager} (it used to be backed by
 * the removed {@code org.openide.util.WeakSet#putIfAbsent}).
 */
public class APTStringManagerTest extends NbTestCase {

    public APTStringManagerTest(String name) {
        super(name);
    }

    private void checkInterning(APTStringManager manager) {
        String first = new String("some_identifier");
        String equal = new String("some_identifier");
        assertNotSame(first, equal);
        assertSame("first instance becomes canonical", first, manager.getString(first));
        assertSame("equal text returns the canonical instance", first, manager.getString(equal));
        CharSequence other = manager.getString(new String("other_identifier"));
        assertEquals("other_identifier", other.toString());
        assertNotSame(first, other);
    }

    public void testSingleInterning() {
        checkInterning(APTStringManager.instance("test-single-" + getName(), APTStringManager.CacheKind.Single));
    }

    public void testSlicedInterning() {
        checkInterning(APTStringManager.instance("test-sliced-" + getName(), APTStringManager.CacheKind.Sliced));
    }

    public void testCanonicalInstanceIsWeak() {
        APTStringManager manager = APTStringManager.instance("test-weak-" + getName(), APTStringManager.CacheKind.Single);
        String text = new String("collectable_identifier");
        manager.getString(text);
        Reference<String> ref = new WeakReference<>(text);
        text = null;
        assertGC("the manager must not keep interned text alive", ref);
        // a new instance is accepted as canonical once the old one is gone
        String again = new String("collectable_identifier");
        assertSame(again, manager.getString(again));
    }

    public void testDispose() {
        APTStringManager manager = APTStringManager.instance("test-dispose-" + getName(), APTStringManager.CacheKind.Single);
        String first = new String("disposed_identifier");
        manager.getString(first);
        manager.dispose();
        String second = new String("disposed_identifier");
        assertSame("after dispose a new instance becomes canonical", second, manager.getString(second));
    }
}
