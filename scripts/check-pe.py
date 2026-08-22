#!/usr/bin/env python3
"""Fail closed unless a file is a native PE32+ x86-64 executable."""

from __future__ import annotations

import argparse
from pathlib import Path
import struct
import sys


DOS_HEADER_BYTES = 64
PE_HEADER_BYTES = 26
MAX_PE_OFFSET = 1_048_576
PE32_PLUS_MAGIC = 0x20B
X86_64_MACHINE = 0x8664


class PeCheckError(ValueError):
    """The input is not a bounded PE32+ x86-64 executable."""


def check(path: Path) -> None:
    try:
        with path.open("rb") as executable:
            dos_header = executable.read(DOS_HEADER_BYTES)
            if len(dos_header) != DOS_HEADER_BYTES or dos_header[:2] != b"MZ":
                raise PeCheckError("missing DOS MZ header")
            pe_offset = struct.unpack_from("<I", dos_header, 0x3C)[0]
            if not DOS_HEADER_BYTES <= pe_offset <= MAX_PE_OFFSET:
                raise PeCheckError("PE header offset is outside the bounded range")
            executable.seek(pe_offset)
            pe_header = executable.read(PE_HEADER_BYTES)
    except OSError as error:
        raise PeCheckError(f"cannot read executable: {error}") from error
    if len(pe_header) != PE_HEADER_BYTES or pe_header[:4] != b"PE\0\0":
        raise PeCheckError("missing PE signature")
    machine = struct.unpack_from("<H", pe_header, 4)[0]
    optional_header_size = struct.unpack_from("<H", pe_header, 20)[0]
    optional_magic = struct.unpack_from("<H", pe_header, 24)[0]
    if machine != X86_64_MACHINE:
        raise PeCheckError(f"machine is 0x{machine:04x}, expected x86-64 0x8664")
    if optional_header_size < 2 or optional_magic != PE32_PLUS_MAGIC:
        raise PeCheckError("optional header is not PE32+")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("executable", type=Path)
    args = parser.parse_args()
    try:
        check(args.executable)
    except PeCheckError as error:
        print(f"PE check failed: {error}", file=sys.stderr)
        return 1
    print(f"PE32+ x86-64 verified: {args.executable}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
