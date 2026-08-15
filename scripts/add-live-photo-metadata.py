#!/usr/bin/env python3
"""Stream a synthetic Apple live-photo content identifier into a JPEG."""

from __future__ import annotations

import argparse
import re
import shutil
import struct
import sys


MAX_IDENTIFIER_BYTES = 128
COPY_BUFFER_BYTES = 65_536


class MetadataError(ValueError):
    """The requested synthetic JPEG metadata is invalid."""


def entry(tag: int, field_type: int, count: int, value: bytes) -> bytes:
    if len(value) != 4:
        raise MetadataError("TIFF entry value must contain four bytes")
    return struct.pack(">HHI", tag, field_type, count) + value


def exif_payload(identifier: str) -> bytes:
    encoded_identifier = identifier.encode("ascii") + b"\0"
    if len(encoded_identifier) > MAX_IDENTIFIER_BYTES:
        raise MetadataError("content identifier exceeds its bound")
    make = b"Apple\0"
    model = b"Synthetic iPhone\0"
    ifd0_offset = 8
    ifd0_size = 2 + 3 * 12 + 4
    make_offset = ifd0_offset + ifd0_size
    model_offset = make_offset + len(make)
    exif_ifd_offset = model_offset + len(model) + 1
    exif_ifd_size = 2 + 12 + 4
    maker_note_offset = exif_ifd_offset + exif_ifd_size
    maker_note_header = b"Apple iOS\0\0\x01MM"
    maker_note_size = len(maker_note_header) + 2 + 12 + 4 + len(encoded_identifier)

    ifd0 = b"".join(
        (
            struct.pack(">H", 3),
            entry(0x010F, 2, len(make), struct.pack(">I", make_offset)),
            entry(0x0110, 2, len(model), struct.pack(">I", model_offset)),
            entry(0x8769, 4, 1, struct.pack(">I", exif_ifd_offset)),
            struct.pack(">I", 0),
        )
    )
    exif_ifd = b"".join(
        (
            struct.pack(">H", 1),
            entry(0x927C, 7, maker_note_size, struct.pack(">I", maker_note_offset)),
            struct.pack(">I", 0),
        )
    )
    maker_note = b"".join(
        (
            maker_note_header,
            struct.pack(">H", 1),
            entry(0x0011, 2, len(encoded_identifier), struct.pack(">I", 32)),
            struct.pack(">I", 0),
            encoded_identifier,
        )
    )
    tiff = b"MM\x00*" + struct.pack(">I", ifd0_offset) + ifd0 + make + model + b"\0" + exif_ifd + maker_note
    payload = b"Exif\0\0" + tiff
    if len(payload) + 2 > 65_535:
        raise MetadataError("EXIF payload exceeds the JPEG APP1 bound")
    return payload


def inject(identifier: str) -> None:
    source = sys.stdin.buffer
    destination = sys.stdout.buffer
    if source.read(2) != b"\xff\xd8":
        raise MetadataError("input is not a JPEG stream")
    payload = exif_payload(identifier)
    destination.write(b"\xff\xd8\xff\xe1")
    destination.write(struct.pack(">H", len(payload) + 2))
    destination.write(payload)
    shutil.copyfileobj(source, destination, length=COPY_BUFFER_BYTES)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--content-identifier", required=True)
    arguments = parser.parse_args()
    if re.fullmatch(r"synthetic-[a-z0-9-]{8,80}", arguments.content_identifier) is None:
        print("live-photo metadata requires a bounded synthetic identifier", file=sys.stderr)
        return 2
    try:
        inject(arguments.content_identifier)
    except (BrokenPipeError, MetadataError, OSError) as failure:
        print(f"live-photo metadata generation failed: {failure}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
