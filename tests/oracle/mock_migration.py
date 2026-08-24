"""Versioned synthetic two-server migration scenario."""

from __future__ import annotations

import base64
import json
from pathlib import Path
from typing import Any, Callable

FIXTURE = Path(__file__).resolve().parent / "server-fixtures/immich-migration-v1.json"


class MigrationFixtureError(ValueError):
    """The synthetic migration fixture is malformed."""


def load_fixture() -> dict[str, Any]:
    try:
        fixture = json.loads(FIXTURE.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise MigrationFixtureError("cannot read migration fixture") from error
    provenance = fixture.get("provenance") if isinstance(fixture, dict) else None
    if (
        fixture.get("schema") != "mock-immich-migration-v1"
        or fixture.get("fixture_id") != "synthetic-immich-migration"
        or not isinstance(provenance, dict)
        or provenance.get("kind") != "synthetic"
        or provenance.get("license") != "CC0-1.0"
    ):
        raise MigrationFixtureError("migration fixture provenance is invalid")
    assets = fixture.get("assets")
    albums = fixture.get("albums")
    if not isinstance(assets, list) or not assets or not isinstance(albums, list):
        raise MigrationFixtureError("migration fixture content is invalid")
    return fixture


def scenario(
    default_scenario: Callable[[], dict[str, Any]],
    api_key: str,
    *,
    source: bool,
) -> dict[str, Any]:
    selected = default_scenario()
    selected["api_key"] = api_key
    if not source:
        return selected
    fixture = load_fixture()
    assets = []
    for value in fixture["assets"]:
        if not isinstance(value, dict) or not isinstance(value.get("body_base64"), str):
            raise MigrationFixtureError("migration asset is invalid")
        try:
            body = base64.b64decode(value["body_base64"], validate=True)
        except (ValueError, TypeError) as error:
            raise MigrationFixtureError("migration body is invalid") from error
        if not body:
            raise MigrationFixtureError("migration body is empty")
        asset = {key: item for key, item in value.items() if key != "body_base64"}
        location = asset.pop("synthetic_location", None)
        if location is not None:
            if (
                not isinstance(location, list)
                or len(location) != 2
                or any(not isinstance(item, (int, float)) for item in location)
            ):
                raise MigrationFixtureError("migration location is invalid")
            asset["latitude"], asset["longitude"] = location
        asset["body"] = body
        assets.append(asset)
    selected["archive_assets"] = assets
    selected["archive_albums"] = [dict(album) for album in fixture["albums"]]
    return selected
