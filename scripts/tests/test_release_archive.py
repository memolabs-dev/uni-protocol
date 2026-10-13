import hashlib
import importlib.util
import io
from pathlib import Path
import tarfile
import tempfile
import unittest
import zipfile

FILE = Path(__file__).resolve().parents[1] / "release_archive.py"
module_spec = importlib.util.spec_from_file_location("release_archive", FILE)
release = importlib.util.module_from_spec(module_spec)
module_spec.loader.exec_module(release)


class ReleaseArchiveTests(unittest.TestCase):
    def test_deterministic_package_and_verified_checksum(self):
        for target in ("x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc"):
            with self.subTest(target=target), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                name = release.member_name(target)
                binary = root / name
                binary.write_bytes(b"test binary" * 11)
                archive = root / f"uni-{target}.tar.gz"
                digest = release.package(binary, archive, target)
                self.assertEqual(digest, hashlib.sha256(archive.read_bytes()).hexdigest())
                self.assertEqual(release.verify(archive, target, binary=binary), digest)
                self.assertEqual(
                    (root / (archive.name + ".sha256")).read_text(),
                    f"{digest}  {archive.name}" + chr(10),
                )
                first = archive.read_bytes()
                release.package(binary, archive, target)
                self.assertEqual(archive.read_bytes(), first, "same binary must package identically")

    def test_rejects_legacy_zip_with_gzip_extension(self):
        with tempfile.TemporaryDirectory() as temp:
            archive = Path(temp) / "uni-x86_64-pc-windows-msvc.tar.gz"
            with zipfile.ZipFile(archive, "w") as zip_file:
                zip_file.writestr("uni.exe", "not a gzip tar")
            with self.assertRaisesRegex(ValueError, "not gzip"):
                release.verify(archive, "x86_64-pc-windows-msvc")

    def test_rejects_archive_payload_mismatch(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary = root / "uni"
            binary.write_bytes(b"before")
            archive = root / "uni-aarch64-apple-darwin.tar.gz"
            release.package(binary, archive, "aarch64-apple-darwin")
            binary.write_bytes(b"after")
            with self.assertRaisesRegex(ValueError, "differ"):
                release.verify(archive, "aarch64-apple-darwin", binary=binary)

    def test_rejects_symlink_or_unsafe_member(self):
        with tempfile.TemporaryDirectory() as temp:
            archive = Path(temp) / "uni-x86_64-unknown-linux-gnu.tar.gz"
            with tarfile.open(archive, "w:gz") as tar:
                item = tarfile.TarInfo("uni")
                item.type = tarfile.SYMTYPE
                item.linkname = "../outside"
                tar.addfile(item)
            with self.assertRaisesRegex(ValueError, "one regular"):
                release.verify(archive, "x86_64-unknown-linux-gnu")
            with tarfile.open(archive, "w:gz") as tar:
                item = tarfile.TarInfo("../uni")
                content = b"payload"
                item.size = len(content)
                tar.addfile(item, io.BytesIO(content))
            with self.assertRaisesRegex(ValueError, "one regular"):
                release.verify(archive, "x86_64-unknown-linux-gnu")


if __name__ == "__main__":
    unittest.main()
