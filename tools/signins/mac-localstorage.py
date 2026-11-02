#!/usr/bin/env python3
"""Copper (Mac) localStorage -> JSON {"<origin>": {"<key>": "<value>"}} on stdout.

Reads WebKit's per-origin stores under
~/Library/WebKit/<bundle>/WebsiteData/Default/<top>/<origin>/: the `origin`
file names the top and frame origins, LocalStorage/localstorage.sqlite3
holds ItemTable (key TEXT, value BLOB as UTF-16LE). Only first-party stores
(frame origin == top origin) come out: Windows' import is top-level only.
Prints a count summary to stderr; never prints a value anywhere but stdout.
Usage: python3 mac-localstorage.py [WebsiteData/Default dir] | <seal on the other side>
"""
import json, os, sqlite3, struct, sys


def read_string(b, off):
    (n,) = struct.unpack_from("<I", b, off)
    eight = b[off + 4]
    off += 5
    if eight:
        return b[off : off + n].decode("latin-1"), off + n
    return b[off : off + 2 * n].decode("utf-16-le"), off + 2 * n


def read_origin(b, off):
    """WebCore SecurityOriginData as WebKit persists it: protocol, host, optional<uint16> port."""
    proto, off = read_string(b, off)
    host, off = read_string(b, off)
    has_port = b[off]
    off += 1
    port = None
    if has_port:
        (port,) = struct.unpack_from("<H", b, off)
        off += 2
    s = f"{proto}://{host}"
    if port is not None:
        s += f":{port}"
    return s, off


def parse_origin_file(b):
    top, off = read_origin(b, 0)
    frame, _ = read_origin(b, off)
    return top, frame


def main():
    root = sys.argv[1] if len(sys.argv) > 1 else os.path.expanduser(
        "~/Library/WebKit/com.collinrijock.copper/WebsiteData/Default")
    out, keys = {}, 0
    for top_dir in sorted(os.listdir(root)):
        if not os.path.isdir(os.path.join(root, top_dir)):
            continue  # `salt`
        for frame_dir in sorted(os.listdir(os.path.join(root, top_dir))):
            d = os.path.join(root, top_dir, frame_dir)
            db = os.path.join(d, "LocalStorage", "localstorage.sqlite3")
            try:
                top, frame = parse_origin_file(open(os.path.join(d, "origin"), "rb").read())
            except (OSError, struct.error, IndexError, UnicodeDecodeError):
                continue
            if top != frame or not frame.startswith(("http://", "https://")) or not os.path.exists(db):
                continue
            # Read-only and immutable: Copper may hold the database open.
            con = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
            try:
                rows = con.execute("SELECT key, value FROM ItemTable").fetchall()
            except sqlite3.Error:
                rows = []
            finally:
                con.close()
            items = {}
            for k, v in rows:
                if isinstance(v, bytes):
                    v = v.decode("utf-16-le", "replace")
                items[k] = v
            if items:
                out[frame] = items
                keys += len(items)
    sys.stderr.write(f"{keys} keys, {len(out)} origins\n")
    json.dump(out, sys.stdout)


if __name__ == "__main__":
    main()
