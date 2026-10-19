"""Packaging regression tests; never use real credentials, photos or Cargo cache."""
import contextlib
import hashlib
import io
import json
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch

import package


class PackagingTests(unittest.TestCase):
    def fixture(self, root):
        (root / "Cargo.toml").write_text('[package]\nversion = "0.1.0-dev"\n')
        for name in (
            "target/opt-dev/rphotos", "target/opt-dev/rphotos.exe", "LICENSE-MIT",
            "LICENSE-APACHE", "README.md", "docs/VERIFICATION.md", "docs/PREVIEW.md",
            "docs/UPSTREAM-README.md", "assets/rphotos.svg", "assets/rphotos.png",
            "docs/PRODUCT.md", "docs/IMPLEMENTATION.md", "docs/PHOTOS-2019.md",
            "docs/AUDIT-FIXES.md",
            "docs/PHOTOS-2019-AUDIT.md", "docs/screenshots/README.md",
            "docs/screenshots/collection-light.png", "docs/screenshots/albums.png",
            "docs/screenshots/viewer-light.png",
            "assets/rphotos.desktop", "Cargo.lock",
            # These must NOT be collected by the archive's strict allowlist.
            "settings.yaml", "private/photo.jpg", "private/hosts.yml",
        ):
            path = root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"synthetic fixture")
        dependency = root / "dependency"
        (dependency / "fonts").mkdir(parents=True)
        (dependency / "Cargo.toml").write_text("synthetic MPL manifest")
        (dependency / "lib.rs").write_text("// synthetic MPL covered source")
        (dependency / "LICENSE").write_text("synthetic MPL license")
        (dependency / "fonts/OFL.txt").write_text("synthetic font attribution")
        return {"packages": [{
            "name": "fixture-dependency", "version": "1.0.0",
            "manifest_path": str(dependency / "Cargo.toml"), "license": "MPL-2.0",
            "repository": "https://example.invalid/fixture", "source": None,
        }]}

    def test_native_archive_checksum_allowlist_and_notices(self):
        for system in ("Linux", "Darwin", "Windows"):
            with self.subTest(system=system), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                metadata = self.fixture(root)
                with patch.object(package.platform, "system", return_value=system), \
                     patch.object(package.platform, "machine", return_value="test-arch"), \
                     patch.object(package, "json_metadata", return_value=metadata), \
                     contextlib.redirect_stdout(io.StringIO()):
                    archive = package.package(root, "opt-dev", root / "dist")
                with archive.open("rb") as file:
                    digest = hashlib.file_digest(file, "sha256").hexdigest()
                self.assertEqual(archive.with_name(archive.name + ".sha256")
                                 .read_text().split()[0], digest)
                if system == "Windows":
                    with zipfile.ZipFile(archive) as out:
                        contents = {n: out.read(n) for n in out.namelist()}
                else:
                    with tarfile.open(archive) as out:
                        contents = {n: out.extractfile(n).read() for n in out.getnames()}
                relative = {n.split("/", 1)[1] for n in contents}
                self.assertEqual(len(relative), 23 if system == "Linux" else 22)
                self.assertIn("rphotos.exe" if system == "Windows" else "rphotos", relative)
                self.assertNotIn("settings.yaml", relative)
                self.assertFalse(any("private" in n or ".." in Path(n).parts for n in contents))
                notices = next(v for n, v in contents.items()
                               if n.endswith("THIRD-PARTY-NOTICES.txt"))
                self.assertIn(b"synthetic MPL covered source", notices)
                self.assertIn(b"synthetic font attribution", notices)
                build = json.loads(next(v for n, v in contents.items()
                                        if n.endswith("BUILD-INFO.json")))
                self.assertEqual(build["binary_sha256"], hashlib.sha256(b"synthetic fixture").hexdigest())
                self.assertIsNone(build["source_commit"])
                self.assertIsNone(build["source_dirty_at_packaging"])
                self.assertEqual(build["profile"], "opt-dev")
                self.assertTrue(build["development_build"])

    def test_missing_input_does_not_create_archive(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.fixture(root)
            # A narrow fixture deletion, never application/user data.
            (root / "LICENSE-MIT").unlink()
            with self.assertRaises(FileNotFoundError):
                package.package(root, "opt-dev", root / "dist")
            self.assertFalse((root / "dist").exists())


if __name__ == "__main__":
    unittest.main()
