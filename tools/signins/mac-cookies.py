#!/usr/bin/env python3
"""Copper (Mac) default cookie jar -> JSON list of copper_engine::Cookie on stdout.

Reads ~/Library/HTTPStorages/<bundle>.binarycookies (WebKit's persisted jar;
session-only cookies are not in it). Expired cookies are skipped. Prints a
count summary to stderr; never prints a value anywhere but stdout.
Usage: python3 mac-cookies.py [path] | <seal on the other side>
"""
import json, os, struct, sys, time

MAC_EPOCH = 978307200  # 2001-01-01 in Unix seconds


def cstr(b, off):
    end = b.index(b"\0", off)
    return b[off:end].decode("utf-8", "replace")


def parse(data):
    if data[:4] != b"cook":
        raise ValueError("not a binarycookies file")
    (n,) = struct.unpack(">i", data[4:8])
    sizes = struct.unpack(">" + "i" * n, data[8 : 8 + 4 * n])
    off = 8 + 4 * n
    for size in sizes:
        page = data[off : off + size]
        off += size
        (count,) = struct.unpack("<i", page[4:8])
        for i in range(count):
            (at,) = struct.unpack("<i", page[8 + 4 * i : 12 + 4 * i])
            c = page[at:]
            flags = struct.unpack("<i", c[8:12])[0]
            u, nm, pa, va = struct.unpack("<iiii", c[16:32])
            expiry = struct.unpack("<d", c[40:48])[0]
            yield {
                "domain": cstr(c, u),
                "name": cstr(c, nm),
                "path": cstr(c, pa) or "/",
                "value": cstr(c, va),
                "expires": int(expiry + MAC_EPOCH),
                "secure": bool(flags & 1),
                "http_only": bool(flags & 4),
            }


def main():
    path = sys.argv[1] if len(sys.argv) > 1 else os.path.expanduser(
        "~/Library/HTTPStorages/com.collinrijock.copper.binarycookies")
    now = time.time()
    out = []
    for c in parse(open(path, "rb").read()):
        if c["expires"] <= now:
            continue
        # The jar keeps no SameSite; a secure cookie gets None (what WebKit
        # honoured), others Lax (Chromium's default).
        c["same_site"] = "none" if c["secure"] else "lax"
        out.append(c)
    sys.stderr.write(f"{len(out)} cookies, {len({c['domain'] for c in out})} sites\n")
    json.dump(out, sys.stdout)


if __name__ == "__main__":
    main()
