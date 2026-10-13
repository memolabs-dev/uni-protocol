#!/usr/bin/env python3
"""Deterministic UNI archive packaging, extraction validation and SHA-256 sidecar.

Packages have one member (uni or uni.exe). This proves that release assets are
readable and byte-equal to the compiled binary, NOT that compilation is
reproducible or that a human signed the release.
"""
import argparse
import gzip
import hashlib
import io
from pathlib import Path
import tarfile
import sys

TARGETS = {
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-pc-windows-msvc",
}


def member_name(target: str) -> str:
    if target not in TARGETS:
        raise ValueError(f"unsupported release target {target}")
    return "uni.exe" if "windows" in target else "uni"


def package(binary: Path, archive: Path, target: str) -> str:
    name = member_name(target)
    if binary.name != name:
        raise ValueError(f"{target} must package {name}, not {binary.name}")
    content = binary.read_bytes()
    info = tarfile.TarInfo(name)
    info.size = len(content)
    info.mode = 0o755
    info.mtime = 0
    info.uid = info.gid = 0
    info.uname = info.gname = ""
    # Gzip header timestamp and original filename are deliberately excluded.
    with archive.open("wb") as raw:
        with gzip.GzipFile(fileobj=raw, mode="wb", filename="", mtime=0) as zipped:
            with tarfile.open(fileobj=zipped, mode="w", format=tarfile.GNU_FORMAT) as tar:
                tar.addfile(info, io.BytesIO(content))
    return verify(archive, target, binary=binary, write_sidecar=True)


def verify(archive: Path, target: str, *, binary: Path | None = None,
           write_sidecar: bool = False) -> str:
    expected = member_name(target)
    if archive.name != f"uni-{target}.tar.gz":
        raise ValueError("release archive filename does not match target")
    with archive.open("rb") as handle:
        if handle.read(2) != bytes.fromhex("1f8b"):
            raise ValueError("archive is not gzip (legacy ZIP disguised as tar.gz?)")
    try:
        with tarfile.open(archive, "r:gz") as tar:
            members = tar.getmembers()
            if len(members) != 1 or members[0].name != expected or not members[0].isfile():
                raise ValueError(f"archive must contain exactly one regular {expected}")
            reader = tar.extractfile(members[0])
            if reader is None:
                raise ValueError("archive member cannot be read")
            archived_data = reader.read()
    except (tarfile.TarError, EOFError) as exc:
        raise ValueError(f"cannot extract gzip tar archive: {exc}") from exc
    if binary is not None and archived_data != binary.read_bytes():
        raise ValueError("archive contents differ from the compiled binary")
    checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
    if write_sidecar:
        Path(str(archive) + ".sha256").write_text(f"{checksum}  {archive.name}" + chr(10))
    return checksum


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=["package", "verify"])
    parser.add_argument("--target", choices=sorted(TARGETS), required=True)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--binary", type=Path)
    args = parser.parse_args()
    try:
        if args.operation == "package":
            if args.binary is None:
                parser.error("package requires --binary")
            checksum = package(args.binary, args.archive, args.target)
        else:
            checksum = verify(args.archive, args.target, binary=args.binary)
        print(f"SHA256 {args.archive.name}: {checksum}")
        return 0
    except (OSError, ValueError) as exc:
        print(f"release verification failed: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
