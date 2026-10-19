#!/usr/bin/env python3
"""Write Copper for Windows' copper-version.json (Linux manifest schema).

    python3 packaging/version-manifest.py --dist dist --version 0.1.0 \
      --tag v0.1.0 --commit <sha> --repo copper-browser/copper-windows \
      --published-at 2026-10-19T00:00:00Z > dist/copper-version.json

The executable becomes CopperSetup.exe solely by renaming it; the zip stores
that same executable as copper.exe with the license and README. A URL and
SHA-256 for each architecture/format lives in `assets`, like copper-linux.
"""

import argparse
import hashlib
import json
from pathlib import Path
import re
import sys
import tempfile

# kind, architecture, name. Filenames stay the same across releases; URL uses
# the tag for an immutable asset and /latest/ for the current stable release.
ASSETS = (
    ("installer", "x64", "CopperSetup-x64.exe"),
    ("installer", "arm64", "CopperSetup-arm64.exe"),
    ("zip", "x64", "copper-windows-x64.zip"),
    ("zip", "arm64", "copper-windows-arm64.zip"),
)


def first_paragraph(path: Path) -> str:
    """Read the first nonempty notes paragraph, as the Linux manifest does."""
    try:
        text = path.read_text(encoding="utf-8")
    except OSError:
        return ""
    for paragraph in re.split(r"\n\s*\n", text):
        if paragraph.strip():
            return " ".join(line.strip() for line in paragraph.splitlines() if line.strip())
    return ""


def sha256(path: Path) -> str:
    """Hash large installers without reading the entire executable into RAM."""
    digest = hashlib.sha256()
    with path.open("rb") as src:
        for chunk in iter(lambda: src.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def build_manifest(
    dist: Path, version: str, tag: str, commit: str, repo: str,
    published_at: str, notes: Path, status: str = "lkg",
) -> dict:
    """Build the shared release schema from the actual bytes staged in dist."""
    if tag != f"v{version}":
        raise ValueError(f"tag {tag!r} does not match version {version!r}")
    base = f"https://github.com/{repo}/releases"
    assets = []
    for kind, arch, name in ASSETS:
        path = dist / name
        if not path.is_file() or path.stat().st_size == 0:
            raise ValueError(f"missing or empty release asset: {path}")
        assets.append(
            {
                "kind": kind,
                "arch": arch,
                "name": name,
                "size": path.stat().st_size,
                "sha256": sha256(path),
                "url": f"{base}/download/{tag}/{name}",
                "latestName": name,
                "latestUrl": f"{base}/latest/download/{name}",
            }
        )
    return {
        "schemaVersion": 1,
        "product": "copper",
        "platform": "windows",
        "version": version,
        "releaseId": tag,
        "commit": commit,
        "source": f"https://github.com/{repo}/commit/{commit}",
        "status": status,
        "publishedAt": published_at,
        "installer": "CopperSetup-x64.exe",
        "latestUrl": f"{base}/latest",
        "notes": first_paragraph(notes),
        "assets": assets,
    }


def self_test() -> None:
    """Exercise schema, hashes, URL routes, and rejection of missing assets."""
    with tempfile.TemporaryDirectory(prefix="copper-windows-manifest-") as root:
        dist = Path(root)
        notes = dist / "NOTES.md"
        notes.write_text("A new browser.\nFor Windows.\n\nMore details.\n", encoding="utf-8")
        for _, arch, name in ASSETS:
            (dist / name).write_bytes(f"copper:{arch}:{name}".encode())
        manifest = build_manifest(
            dist, "0.1.0", "v0.1.0", "abcd1234", "copper-browser/copper-windows",
            "2026-10-19T00:00:00Z", notes,
        )
        assert set(manifest) == {
            "schemaVersion", "product", "platform", "version", "releaseId",
            "commit", "source", "status", "publishedAt", "installer",
            "latestUrl", "notes", "assets",
        }
        assert manifest["schemaVersion"] == 1
        assert manifest["platform"] == "windows"
        assert manifest["notes"] == "A new browser. For Windows."
        assert manifest["installer"] == "CopperSetup-x64.exe"
        assert len(manifest["assets"]) == 4
        assert {(a["kind"], a["arch"]) for a in manifest["assets"]} == {
            ("installer", "x64"), ("installer", "arm64"),
            ("zip", "x64"), ("zip", "arm64"),
        }
        for asset in manifest["assets"]:
            data = (dist / asset["name"]).read_bytes()
            assert asset["size"] == len(data)
            assert asset["sha256"] == hashlib.sha256(data).hexdigest()
            assert asset["url"].endswith(f"/download/v0.1.0/{asset['name']}")
            assert asset["latestUrl"].endswith(f"/latest/download/{asset['name']}")
        (dist / ASSETS[0][2]).unlink()
        try:
            build_manifest(
                dist, "0.1.0", "v0.1.0", "abcd1234", "copper-browser/copper-windows",
                "2026-10-19T00:00:00Z", notes,
            )
        except ValueError as exc:
            assert "missing or empty release asset" in str(exc)
        else:
            raise AssertionError("missing installer was accepted")
    print("version-manifest self-test: OK (4 assets, SHA-256, URLs, missing asset)")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--dist", type=Path)
    parser.add_argument("--version")
    parser.add_argument("--tag")
    parser.add_argument("--commit")
    parser.add_argument("--repo")
    parser.add_argument("--published-at")
    parser.add_argument("--notes", type=Path, default=Path("NOTES.md"))
    parser.add_argument("--status", default="lkg")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return 0
    for key in ("dist", "version", "tag", "commit", "repo", "published_at"):
        if getattr(args, key) is None:
            parser.error(f"--{key.replace('_', '-')} is required")
    try:
        manifest = build_manifest(
            args.dist, args.version, args.tag, args.commit, args.repo,
            args.published_at, args.notes, args.status,
        )
    except ValueError as exc:
        print(f"version-manifest: {exc}", file=sys.stderr)
        return 1
    json.dump(manifest, sys.stdout, indent=2)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
