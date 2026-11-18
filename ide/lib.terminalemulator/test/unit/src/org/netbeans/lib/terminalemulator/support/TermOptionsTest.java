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

package org.netbeans.lib.terminalemulator.support;

import java.awt.Color;
import java.util.HashMap;
import java.util.Map;
import java.util.prefs.AbstractPreferences;
import java.util.prefs.Preferences;
import org.netbeans.junit.NbTestCase;

public class TermOptionsTest extends NbTestCase {

    private static final String BACKGROUND = "term.background"; // NOI18N
    private static final String FOREGROUND = "term.foreground"; // NOI18N
    private static final String SELECTION = "term.selectionBackground"; // NOI18N

    public TermOptionsTest(String name) {
        super(name);
    }

    /**
     * Applying an unrelated option (font size) must not freeze the
     * theme's colors into the preferences; otherwise the terminal stays
     * white after switching from a light to a dark look and feel.
     */
    public void testDefaultColorsAreNotStored() {
        Preferences prefs = new MemoryPreferences();
        TermOptions options = TermOptions.getDefault(new MemoryPreferences()).makeCopy();
        options.resetToDefault();
        options.setFontSize(options.getFontSize() + 2);

        options.storeTo(prefs);

        assertNull(prefs.get(BACKGROUND, null));
        assertNull(prefs.get(FOREGROUND, null));
        assertNull(prefs.get(SELECTION, null));
        assertEquals(options.getFontSize(), prefs.getInt("term.fontSize", -1));
    }

    public void testCustomColorIsStoredAndRestoreRemovesIt() {
        Preferences prefs = new MemoryPreferences();
        TermOptions options = TermOptions.getDefault(new MemoryPreferences()).makeCopy();
        options.resetToDefault();
        Color custom = new Color(options.getBackground().getRGB() ^ 0x00FFFFFF);
        options.setBackground(custom);

        options.storeTo(prefs);
        assertEquals(custom.getRGB(), prefs.getInt(BACKGROUND, 0));

        // "Restore" in Tools > Options > Terminal, then Apply
        options.resetToDefault();
        options.storeTo(prefs);
        assertNull(prefs.get(BACKGROUND, null));
    }

    private static final class MemoryPreferences extends AbstractPreferences {

        private final Map<String, String> values = new HashMap<>();

        MemoryPreferences() {
            super(null, "");
        }

        @Override
        protected void putSpi(String key, String value) {
            values.put(key, value);
        }

        @Override
        protected String getSpi(String key) {
            return values.get(key);
        }

        @Override
        protected void removeSpi(String key) {
            values.remove(key);
        }

        @Override
        protected void removeNodeSpi() {
            values.clear();
        }

        @Override
        protected String[] keysSpi() {
            return values.keySet().toArray(new String[0]);
        }

        @Override
        protected String[] childrenNamesSpi() {
            return new String[0];
        }

        @Override
        protected AbstractPreferences childSpi(String name) {
            throw new UnsupportedOperationException();
        }

        @Override
        protected void syncSpi() {
        }

        @Override
        protected void flushSpi() {
        }
    }
}
