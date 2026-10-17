#!/usr/bin/env python3
# Licensed to the Apache Software Foundation (ASF) under one
# or more contributor license agreements.  See the NOTICE file
# distributed with this work for additional information
# regarding copyright ownership.  The ASF licenses this file
# to you under the Apache License, Version 2.0 (the
# "License"); you may not use this file except in compliance
# with the License.  You may obtain a copy of the License at
#
#   http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing,
# software distributed under the License is distributed on an
# "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
# KIND, either express or implied.  See the License for the
# specific language governing permissions and limitations
# under the License.

"""Differential test: run the same request scripts through the C fs_server and the
Rust fs_server and compare their stdout byte for byte (after masking the fields that
legitimately differ: server version, and inode/device/mtime where fixtures are copies).

Usage: compare_with_c.py <c-fs_server> <rust-fs_server> [workdir]
"""

import os
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time

C_BIN, RS_BIN = sys.argv[1], sys.argv[2]
WORK = sys.argv[3] if len(sys.argv) > 3 else tempfile.mkdtemp(prefix="fss-cmp-")
FAILURES = []


def make_fixture(root):
    """A tree with the awkward cases: links, unicode, blanks, backslash, newline, fifo, no-access."""
    os.makedirs(root)
    os.makedirs(f"{root}/sub/deeper/deepest")
    os.makedirs(f"{root}/empty")
    os.makedirs(f"{root}/sp ace dir")
    files = {
        "plain.txt": b"hello\n",
        "sub/a.c": b"int main(){return 0;}\n",
        "sub/deeper/b.h": b"#pragma once\n",
        "sub/deeper/deepest/c.txt": b"x" * 5000,
        "sp ace dir/f i l e": b"blank names\n",
        "unicodé-фыва-😀.txt": b"utf8\n",
        "back\\slash": b"bs\n",
        "new\nline": b"nl\n",
        "exec.sh": b"#!/bin/sh\n",
        "noaccess": b"secret\n",
    }
    for name, data in files.items():
        with open(f"{root}/{name}", "wb") as f:
            f.write(data)
    os.chmod(f"{root}/exec.sh", 0o755)
    os.chmod(f"{root}/noaccess", 0o000)
    os.symlink("plain.txt", f"{root}/link-to-plain")
    os.symlink("does-not-exist", f"{root}/dangling")
    os.symlink("sub", f"{root}/link-to-dir")
    os.mkfifo(f"{root}/fifo")
    # fixed mtimes so copies of the fixture compare equal
    for dirpath, dirnames, filenames in os.walk(root, topdown=False):
        for n in filenames + dirnames:
            p = os.path.join(dirpath, n)
            if not os.path.islink(p):
                os.utime(p, (1_600_000_000.123, 1_600_000_000.123))
    os.utime(root, (1_600_000_000, 1_600_000_000))


def esc(path):
    return path.replace("\\", "\\\\").replace("\n", "\\n")


def req(kind, rid, path, path2=None):
    e = esc(path)
    if path2 is None:
        return f"{kind} {rid} {len(e)} {e}\n"
    e2 = esc(path2)
    return f"{kind} {rid} {len(e)} {e} {len(e2)} {e2}\n"


def run(binary, args, requests, pause_before_quit=0.3, timeout=30):
    """Feeds requests, waits a moment for workers, sends quit; returns (rc, stdout, stderr)."""
    p = subprocess.Popen([binary] + args, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.PIPE)
    # drain both pipes while we write, or a full pipe stalls the server before it sees 'q'
    chunks = {"out": [], "err": []}
    readers = [threading.Thread(target=lambda s=s, k=k: chunks[k].extend(iter(lambda: s.read(65536), b"")))
               for s, k in ((p.stdout, "out"), (p.stderr, "err"))]
    for t in readers:
        t.start()
    try:
        p.stdin.write("".join(requests).encode())
        p.stdin.flush()
        time.sleep(pause_before_quit)
        p.stdin.write(b"q 0 0 \n")
        p.stdin.close()
    except BrokenPipeError:
        pass
    p.wait(timeout=timeout)
    for t in readers:
        t.join(timeout)
    return p.returncode, b"".join(chunks["out"]), b"".join(chunks["err"])


def split_fields(line):
    """Parses 'e <id> <entry>' lines into a list of fields (names may contain blanks)."""
    if not line.startswith(b"e "):
        return None
    s = line.decode("utf-8", "surrogateescape")
    _, rid, rest = s.split(" ", 2)
    n, rest = rest.split(" ", 1)
    name = rest[: int(n)]
    rest = rest[int(n) + 1:]
    t, size, mtime, acc, dev, ino, ln, link = rest.split(" ", 7)
    return [rid, name, t, size, mtime, acc, dev, ino, ln, link]


def normalize(out, mask_ids=False, roots=()):
    lines = []
    for line in out.split(b"\n"):
        for r, placeholder in roots:
            line = line.replace(r.encode(), placeholder.encode())
        if line.startswith(b"i "):
            line = b" ".join(line.split(b" ")[:2]) + b" <version>"
        f = split_fields(line)
        if f is not None and mask_ids:
            f[4] = "<mtime>"
            f[6] = "<dev>"
            f[7] = "<ino>"
            line = ("e " + " ".join(f)).encode("utf-8", "surrogateescape")
        lines.append(line)
    return b"\n".join(lines)


def compare(name, c_out, r_out):
    if c_out == r_out:
        print(f"PASS {name}")
        return
    FAILURES.append(name)
    print(f"FAIL {name}")
    cl, rl = c_out.split(b"\n"), r_out.split(b"\n")
    for i in range(max(len(cl), len(rl))):
        a = cl[i] if i < len(cl) else b"<missing>"
        b = rl[i] if i < len(rl) else b"<missing>"
        if a != b:
            print(f"   line {i}:\n     C : {a!r}\n     RS: {b!r}")
            break


def scenario_readonly():
    fx = f"{WORK}/ro"
    make_fixture(fx)
    rid = iter(range(1, 10000))
    reqs = [
        req("i", next(rid), ""),
        "?\n",
        req("l", next(rid), fx),
        req("r", next(rid), fx),
        req("S", next(rid), f"{fx}/plain.txt"),
        req("S", next(rid), f"{fx}/link-to-plain"),
        req("S", next(rid), f"{fx}/dangling"),
        req("s", next(rid), f"{fx}/link-to-plain"),
        req("s", next(rid), f"{fx}/missing"),
        req("S", next(rid), f"{fx}/missing"),
        req("l", next(rid), f"{fx}/missing-dir"),
        req("l", next(rid), f"{fx}/plain.txt"),
        req("s", next(rid), f"{fx}/new\nline"),
        req("S", next(rid), f"{fx}/unicodé-фыва-😀.txt"),
        req("l", next(rid), f"{fx}/sp ace dir"),
        req("o", next(rid), "access=fast"),
        req("l", next(rid), fx),
        req("o", next(rid), f"dirs-forbidden-to-stat={fx}/sub:{fx}/empty"),
        req("l", next(rid), fx),
        req("S", next(rid), f"{fx}/sub"),
        req("o", next(rid), "access=full"),
    ]
    outs = {}
    for label, binary in (("C", C_BIN), ("R", RS_BIN)):
        cache = f"{WORK}/cache-ro-{label}"
        rc, out, err = run(binary, ["-t", "1", "-p", "-d", cache], reqs)
        outs[label] = normalize(out)
        known_c_bug = label == "C" and b"double free" in err  # C free_settings() frees shared lists
        if rc != 0 and not known_c_bug:
            FAILURES.append(f"readonly rc {label}={rc}")
            print(f"FAIL readonly: {label} exited {rc}: {err.decode(errors='replace')[-500:]}")
    compare("read-only requests (single thread)", outs["C"], outs["R"])
    return fx


def scenario_threads(fx):
    """Many concurrent ls requests with 8 threads: same set of lines, nothing garbled."""
    reqs = []
    rid = 1
    for _ in range(20):
        for d in (fx, f"{fx}/sub", f"{fx}/sub/deeper", f"{fx}/sp ace dir"):
            reqs.append(req("l", rid, d))
            rid += 1
        reqs.append(req("r", rid, fx))
        rid += 1
    outs = {}
    for label, binary in (("C", C_BIN), ("R", RS_BIN)):
        cache = f"{WORK}/cache-mt-{label}"
        _, out, _ = run(binary, ["-t", "8", "-p", "-d", cache], reqs, pause_before_quit=5)
        outs[label] = b"\n".join(sorted(normalize(out).split(b"\n")))
    compare("concurrent requests (8 threads, line multiset)", outs["C"], outs["R"])


def scenario_mutations():
    rid = iter(range(1, 10000))
    script = [
        ("C", "plain.txt", "plain-copy.txt"),
        ("C", "sub", "sub-copy"),
        ("C", "link-to-plain", "link-copy"),
        ("C", "plain.txt", "exec.sh"),          # target exists
        ("C", "missing", "x"),                  # source missing
        ("C", "fifo", "fifo-copy"),             # special file
        ("m", "plain-copy.txt", "moved.txt"),
        ("m", "sub-copy", "no"),                # directory
        ("d", "moved.txt", None),
        ("d", "sub-copy", None),
        ("d", "missing", None),
        ("s", "link-copy", None),
    ]
    outs = {}
    for label, binary in (("C", C_BIN), ("R", RS_BIN)):
        fx = f"{WORK}/mut-{label}"
        make_fixture(fx)
        reqs = [req("l", next(rid), fx)]
        for kind, a, b in script:
            reqs.append(req(kind, next(rid), f"{fx}/{a}", f"{fx}/{b}" if b else None))
        reqs.append(req("l", next(rid), fx))
        rid = iter(range(1, 10000))
        cache = f"{WORK}/cache-mut-{label}"
        _, out, _ = run(binary, ["-t", "1", "-p", "-d", cache], reqs)
        outs[label] = normalize(out, mask_ids=True, roots=[(fx, "<FX>")])
    compare("copy / move / delete", outs["C"], outs["R"])


def scenario_refresh():
    outs = {}
    for label, binary in (("C", C_BIN), ("R", RS_BIN)):
        fx = f"{WORK}/ref-{label}"
        make_fixture(fx)
        cache = f"{WORK}/cache-ref-{label}"
        p = subprocess.Popen([binary, "-t", "1", "-p", "-d", cache], stdin=subprocess.PIPE,
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        w = p.stdin

        def send(s):
            w.write(s.encode())
            w.flush()

        send(req("l", 1, fx))
        send(req("l", 2, f"{fx}/sub"))
        send(req("l", 3, f"{fx}/empty"))
        time.sleep(0.3)
        send(req("R", 4, fx))                      # nothing changed: header + end only
        time.sleep(0.3)
        with open(f"{fx}/sub/new-file", "w") as f:
            f.write("n")
        os.rmdir(f"{fx}/empty")
        time.sleep(0.05)
        send(req("R", 5, fx))                      # change in sub; empty is removed
        time.sleep(0.3)
        send(req("R", 6, fx))                      # REFRESH_SENT already, explicit refresh still reports
        time.sleep(0.3)
        send(req("l", 7, f"{fx}/sub"))
        send(req("R", 8, f"{fx}/sub"))              # after a fresh ls: no change
        time.sleep(0.3)
        send("q 0 0 \n")
        w.close()
        p.stdin = None
        out, _ = p.communicate(timeout=30)
        outs[label] = normalize(out, mask_ids=True, roots=[(fx, "<FX>")])
        with open(f"{cache}/dirtab", "rb") as f:
            outs[label + "-dirtab"] = f.read().replace(fx.encode(), b"<FX>")
    compare("refresh (R) notifications", outs["C"], outs["R"])
    compare("dirtab file after refresh", outs["C-dirtab"], outs["R-dirtab"])


def scenario_cache_interop():
    """Rust must read a cache written by C (and vice versa) and see no changes."""
    fx = f"{WORK}/interop"
    make_fixture(fx)
    for writer, reader in ((C_BIN, RS_BIN), (RS_BIN, C_BIN)):
        cache = f"{WORK}/cache-interop-{os.path.basename(writer)}-{len(writer)}"
        shutil.rmtree(cache, ignore_errors=True)
        run(writer, ["-t", "1", "-p", "-d", cache], [req("l", 1, fx), req("l", 2, f"{fx}/sub")])
        _, out, err = run(reader, ["-t", "1", "-p", "-d", cache], [req("R", 3, fx)])
        name = f"cache written by {'C' if writer == C_BIN else 'RS'} read by {'C' if reader == C_BIN else 'RS'}"
        expected = f"R 3 {len(fx)} {fx}\nx 3 {len(fx)} {fx}\n".encode()
        compare(name, expected, out)


def scenario_ide_command_lines():
    """Exactly what FSSDispatcher passes; must start, answer the handshake and quit with 0."""
    variants = [
        ["-t", "4", "-p", "-d", "{cache}"],
        ["-t", "4", "-p", "-d", "{cache}", "-r", "2", "-v", "1", "-l", "-e", "-c", "-t", "8", "-K", "3000"],
        ["-t", "4", "-p", "-d", "{cache}", "-e", "{cache}-err.txt", "-K", "12345:3000"],
    ]
    for i, v in enumerate(variants):
        cache = f"{WORK}/cache-cli-{i}"
        args = [a.format(cache=cache) for a in v]
        rc, out, err = run(RS_BIN, args, [req("i", 1, "")])
        ok = rc == 0 and out.startswith(b"i 1 1.12.")
        print(f"{'PASS' if ok else 'FAIL'} IDE command line #{i}: rc={rc} out={out[:40]!r}")
        if not ok:
            FAILURES.append(f"cli #{i}")
            print("   stderr:", err.decode(errors="replace")[-800:])


def scenario_lock():
    cache = f"{WORK}/cache-lock"
    a = subprocess.Popen([RS_BIN, "-t", "2", "-p", "-d", cache], stdin=subprocess.PIPE,
                         stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    time.sleep(0.3)
    rc, _, _ = run(RS_BIN, ["-t", "2", "-p", "-d", cache], [])
    ok = rc == 216
    print(f"{'PASS' if ok else 'FAIL'} second server on same cache exits 216 (got {rc})")
    if not ok:
        FAILURES.append("lock")
    rc, out, _ = run(RS_BIN, ["-t", "2", "-p", "-d", cache, "-K", f"{a.pid}:2000"], [req("i", 1, "")])
    a.wait(timeout=5)
    ok = rc == 0 and out.startswith(b"i 1 ") and a.returncode != 0
    print(f"{'PASS' if ok else 'FAIL'} -K <pid>:<msec> takes over the lock (rc={rc}, old rc={a.returncode})")
    if not ok:
        FAILURES.append("lock -K")
    # C holds the lock, Rust must see it (cross-implementation lockf compatibility)
    c = subprocess.Popen([C_BIN, "-t", "2", "-p", "-d", cache], stdin=subprocess.PIPE,
                         stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    time.sleep(0.3)
    rc, _, _ = run(RS_BIN, ["-t", "2", "-p", "-d", cache], [])
    c.send_signal(signal.SIGINT)
    c.wait(timeout=5)
    ok = rc == 216
    print(f"{'PASS' if ok else 'FAIL'} Rust respects a lock held by the C server (got {rc})")
    if not ok:
        FAILURES.append("lock interop")


def scenario_delete_on_disconnect():
    fx = f"{WORK}/dod"
    os.makedirs(fx)
    open(f"{fx}/tmpfile", "w").close()
    run(RS_BIN, ["-t", "2", "-p", "-d", f"{WORK}/cache-dod"], [req("D", 0, f"{fx}/tmpfile")])
    ok = not os.path.exists(f"{fx}/tmpfile")
    print(f"{'PASS' if ok else 'FAIL'} delete-on-disconnect removes the file at exit")
    if not ok:
        FAILURES.append("delete on disconnect")


def main():
    os.makedirs(WORK, exist_ok=True)
    print(f"workdir: {WORK}")
    fx = scenario_readonly()
    scenario_threads(fx)
    scenario_mutations()
    scenario_refresh()
    scenario_cache_interop()
    scenario_ide_command_lines()
    scenario_lock()
    scenario_delete_on_disconnect()
    print()
    if FAILURES:
        print(f"{len(FAILURES)} FAILURE(S): {', '.join(FAILURES)}")
        sys.exit(1)
    print("ALL PASSED")


if __name__ == "__main__":
    main()
