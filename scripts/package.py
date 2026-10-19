#!/usr/bin/env python3
"""Package an already-built native binary, without publishing/installing anything.

Run after cargo build --release (or --profile opt-dev). Requires Python 3.11+.
Archives are portable build artifacts, not signed installers/AppImage/.app bundles.
"""
import argparse
import hashlib
import io
import json
import platform
import re
import subprocess
import tarfile
import zipfile
from pathlib import Path


def package(root, profile, output):
    system = platform.system().lower()
    arch = platform.machine().lower()
    manifest = (root / "Cargo.toml").read_text(encoding="utf-8")
    version = re.search(r'^version\s*=\s*"([^"]+)"', manifest, re.MULTILINE).group(1)
    name = f"rphotos-{version}-{system}-{arch}"
    binary = "rphotos.exe" if system == "windows" else "rphotos"
    files = [(root / "target" / profile / binary, binary)]
    files.extend((root / file, file) for file in (
        "LICENSE-MIT", "LICENSE-APACHE", "README.md", "docs/VERIFICATION.md", "docs/PREVIEW.md",
        "docs/UPSTREAM-README.md", "assets/rphotos.svg", "assets/rphotos.png",
        "docs/PRODUCT.md", "docs/IMPLEMENTATION.md", "docs/PHOTOS-2019.md",
        "docs/AUDIT-FIXES.md",
        "docs/PHOTOS-2019-AUDIT.md", "docs/screenshots/README.md",
        "docs/screenshots/collection-light.png", "docs/screenshots/albums.png",
        "docs/screenshots/viewer-light.png",
        "Cargo.lock",
    ))
    if system == "linux":
        files.append((root / "assets/rphotos.desktop", "assets/rphotos.desktop"))
    # Validate first, so a missing build cannot leave an apparently valid archive.
    for path, _ in files:
        if not path.is_file():
            raise FileNotFoundError(f"Missing package input: {path}")
    output.mkdir(parents=True, exist_ok=True)
    archive = output / f"{name}{'.zip' if system == 'windows' else '.tar.gz'}"
    notice = (
        "Rphotos development build — not a stable release.\n"
        "Based on ViewSkater (MIT OR Apache-2.0). License texts included.\n"
        "Third-party Rust dependencies retain their respective licenses;\n"
        "see THIRD-PARTY-NOTICES.txt and Cargo.lock in this archive.\n"
        "See docs/VERIFICATION.md for verified features and known limitations.\n"
        "No models, photo library, credentials, or personal images are included.\n"
    ).encode()
    metadata = json_metadata(root)
    licenses = [
        "Rphotos third-party dependency inventory (Cargo.lock, all target platforms).",
        "This is a conservative superset, including build/dev/unused target crates.",
        "Original source URLs are provided; option-ext MPL-2.0 source is also included below.",
        "No third-party source modifications are introduced by this packaging script.",
        "",
    ]
    for dependency in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
        if dependency["name"] == "rphotos":
            continue
        directory = Path(dependency["manifest_path"]).parent
        licenses.extend((
            f"\n{'=' * 72}\n{dependency['name']} {dependency['version']}",
            f"License expression: {dependency.get('license') or 'unspecified'}",
            f"Repository: {dependency.get('repository') or ''}",
            f"Source: {dependency.get('source') or ''}",
        ))
        found = []
        for path in sorted(directory.rglob("*")):
            is_font_notice = path.parent.name == "fonts" and path.suffix.lower() == ".txt"
            if path.is_file() and (is_font_notice or any(token in path.name.lower() for token in ("license", "licence", "copying", "copyright"))):
                if path.stat().st_size <= 1024 * 1024:
                    found.append(path)
        for path in found:
            licenses.extend((f"--- {path.relative_to(directory)} ---", path.read_text(encoding="utf-8", errors="replace")))
        if not found:
            licenses.append("Crate does not bundle a separate license text; see the source/repository above.")
        if "MPL-2.0" in (dependency.get("license") or ""):
            # Currently option-ext; include the actual locked covered source,
            # rather than relying only on a mutable upstream URL/source offer.
            for path in sorted(directory.rglob("*")):
                if path.is_file() and (path.suffix == ".rs" or path.name in ("Cargo.toml", "README.md")):
                    licenses.extend((f"--- MPL covered source: {path.relative_to(directory)} ---", path.read_text(encoding="utf-8", errors="replace")))
    third_party = "\n".join(licenses).encode("utf-8")
    with files[0][0].open("rb") as file:
        binary_digest = hashlib.file_digest(file, "sha256").hexdigest()
    try:
        commit = subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=root, stderr=subprocess.DEVNULL,
        ).decode("ascii").strip()
        dirty = bool(subprocess.check_output(
            ["git", "status", "--porcelain", "--untracked-files=no"],
            cwd=root, stderr=subprocess.DEVNULL,
        ).strip())
    except (OSError, subprocess.CalledProcessError):
        commit = None
        dirty = None
    build_info = json.dumps({
        "application": "Rphotos", "version": version, "profile": profile,
        "os": system, "architecture": arch, "source_commit": commit,
        "source_dirty_at_packaging": dirty,
        "binary_sha256": binary_digest,
        "development_build": True,
    }, indent=2).encode("utf-8")
    if system == "windows":
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as out:
            for path, relative in files:
                out.write(path, f"{name}/{relative}")
            out.writestr(f"{name}/BUILD-NOTICE.txt", notice)
            out.writestr(f"{name}/THIRD-PARTY-NOTICES.txt", third_party)
            out.writestr(f"{name}/BUILD-INFO.json", build_info)
    else:
        with tarfile.open(archive, "w:gz") as out:
            for path, relative in files:
                out.add(path, arcname=f"{name}/{relative}", recursive=False)
            info = tarfile.TarInfo(f"{name}/BUILD-NOTICE.txt")
            info.size = len(notice)
            info.mode = 0o644
            out.addfile(info, io.BytesIO(notice))
            info = tarfile.TarInfo(f"{name}/THIRD-PARTY-NOTICES.txt")
            info.size = len(third_party)
            info.mode = 0o644
            out.addfile(info, io.BytesIO(third_party))
            info = tarfile.TarInfo(f"{name}/BUILD-INFO.json")
            info.size = len(build_info)
            info.mode = 0o644
            out.addfile(info, io.BytesIO(build_info))
    with archive.open("rb") as file:
        digest = hashlib.file_digest(file, "sha256").hexdigest()
    archive.with_name(archive.name + ".sha256").write_text(f"{digest}  {archive.name}\n", encoding="ascii")
    print(archive)
    return archive


def json_metadata(root):
    return json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--format-version", "1"], cwd=root,
    ))


if __name__ == "__main__":
    root = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", choices=("release", "opt-dev", "debug"), default="release")
    parser.add_argument("--output", type=Path, default=root / "dist")
    args = parser.parse_args()
    package(root, args.profile, args.output.resolve())
