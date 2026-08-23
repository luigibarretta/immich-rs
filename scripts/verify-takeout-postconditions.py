#!/usr/bin/env python3
"""Verify exact synthetic Takeout metadata and album postconditions."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import sys
from typing import Any


class PostconditionError(ValueError):
    """The disposable Immich state does not match the synthetic fixture."""


EXPECTED = {
    "alpha.png": ("synthetic alpha description", "2024-01-01T00:00:00Z", 3.5, -4.25),
    "beta.png": ("synthetic beta description", "2024-01-02T00:00:00Z", None, None),
    "café.png": ("synthetic unicode description", "2024-01-03T00:00:00Z", None, None),
}


def arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--search", type=Path, required=True)
    parser.add_argument("--albums", type=Path, required=True)
    parser.add_argument("--album-assets", type=Path, required=True)
    return parser.parse_args()


def load(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise PostconditionError(f"cannot load disposable response: {error}") from error


def object_value(value: object, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise PostconditionError(f"{label} must be an object")
    return value


def canonical_instant(value: object) -> str:
    if not isinstance(value, str):
        raise PostconditionError("capture instant must be a string")
    try:
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError as error:
        raise PostconditionError("capture instant is invalid") from error
    return parsed.astimezone(timezone.utc).isoformat(timespec="seconds").replace("+00:00", "Z")


def coordinate(value: object) -> float | None:
    if value is None:
        return None
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise PostconditionError("coordinate must be numeric or null")
    return float(value)


def verify_assets(search: object) -> dict[str, str]:
    assets = object_value(object_value(search, "search").get("assets"), "search assets")
    items = assets.get("items")
    if not isinstance(items, list) or len(items) != len(EXPECTED):
        raise PostconditionError("disposable asset count drifted")
    identifiers: dict[str, str] = {}
    for raw in items:
        item = object_value(raw, "asset")
        name = item.get("originalFileName")
        identifier = item.get("id")
        if name not in EXPECTED or not isinstance(identifier, str) or not identifier:
            raise PostconditionError("unexpected disposable asset")
        exif = object_value(item.get("exifInfo"), "asset EXIF")
        description, instant, latitude, longitude = EXPECTED[name]
        observed = (
            exif.get("description"),
            canonical_instant(exif.get("dateTimeOriginal")),
            coordinate(exif.get("latitude")),
            coordinate(exif.get("longitude")),
        )
        if observed != (description, instant, latitude, longitude):
            raise PostconditionError("normalized metadata postcondition drifted")
        if name in identifiers:
            raise PostconditionError("duplicate disposable filename")
        identifiers[name] = identifier
    if set(identifiers) != set(EXPECTED):
        raise PostconditionError("synthetic asset set drifted")
    return identifiers


def verify_album(albums: object, album_assets: object, identifiers: dict[str, str]) -> None:
    if not isinstance(albums, list) or len(albums) != 1:
        raise PostconditionError("exact-name album lookup did not return one album")
    listed = object_value(albums[0], "album")
    if listed.get("albumName") != "Synthetic Album":
        raise PostconditionError("album name drifted")
    if listed.get("assetCount") != 1:
        raise PostconditionError("album asset count drifted")
    members = object_value(
        object_value(album_assets, "album search").get("assets"),
        "album search assets",
    ).get("items")
    if not isinstance(members, list):
        raise PostconditionError("album members are missing")
    member_ids = [object_value(member, "album member").get("id") for member in members]
    if member_ids != [identifiers["alpha.png"]]:
        raise PostconditionError("album membership drifted")


def main() -> int:
    options = arguments()
    try:
        identifiers = verify_assets(load(options.search))
        verify_album(load(options.albums), load(options.album_assets), identifiers)
    except PostconditionError as error:
        print(f"Takeout postcondition check failed: {error}", file=sys.stderr)
        return 1
    print(json.dumps({
        "schema": "phase8-takeout-postconditions-v1",
        "assets": 3,
        "metadata_assignments": 3,
        "albums": 1,
        "album_memberships": 1,
        "exact": True,
    }, sort_keys=True, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
