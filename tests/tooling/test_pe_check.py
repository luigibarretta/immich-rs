"""Tests for bounded PE32+ x86-64 identification."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import struct
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("check_pe", ROOT / "scripts" / "check-pe.py")
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("cannot load check-pe.py")
CHECK_PE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = CHECK_PE
SPEC.loader.exec_module(CHECK_PE)


def synthetic_pe(machine: int = 0x8664, magic: int = 0x20B) -> bytes:
    executable = bytearray(0x80)
    executable[:2] = b"MZ"
    struct.pack_into("<I", executable, 0x3C, 0x80)
    executable.extend(b"PE\0\0")
    executable.extend(struct.pack("<HHIIIHHH", machine, 1, 0, 0, 0, 2, 0, magic))
    return bytes(executable)


class PeCheckTests(unittest.TestCase):
    def check_bytes(self, content: bytes) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "test.exe"
            path.write_bytes(content)
            CHECK_PE.check(path)

    def test_accepts_pe32_plus_x86_64(self) -> None:
        self.check_bytes(synthetic_pe())

    def test_rejects_non_x86_64_machine(self) -> None:
        with self.assertRaisesRegex(CHECK_PE.PeCheckError, "expected x86-64"):
            self.check_bytes(synthetic_pe(machine=0x014C))

    def test_rejects_pe32_optional_header(self) -> None:
        with self.assertRaisesRegex(CHECK_PE.PeCheckError, "not PE32\\+"):
            self.check_bytes(synthetic_pe(magic=0x10B))

    def test_rejects_truncated_input(self) -> None:
        with self.assertRaisesRegex(CHECK_PE.PeCheckError, "missing DOS"):
            self.check_bytes(b"MZ")


if __name__ == "__main__":
    unittest.main()
