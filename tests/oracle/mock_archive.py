"""Synthetic read-only archive responses for the bounded Immich mock."""

from __future__ import annotations

import base64
import hashlib
from typing import Any


def archive_search_response(
    scenario: dict[str, Any], request: object
) -> dict[str, object]:
    """Return one deterministic metadata-search page from synthetic bytes."""
    if not isinstance(request, dict):
        return {"assets": {"items": [], "count": 0, "total": 0}}
    page = request.get("page")
    size = request.get("size")
    visibility = request.get("visibility")
    include_trashed = "trashedAfter" not in request
    if (
        not isinstance(page, int)
        or page < 1
        or not isinstance(size, int)
        or not 1 <= size <= 1_000
        or visibility not in {"timeline", "archive", "hidden"}
    ):
        return {"assets": {"items": [], "count": 0, "total": 0}}
    selected = [
        asset
        for asset in _assets(scenario)
        if asset["visibility"] == visibility
        and (include_trashed or not asset.get("trashed", False))
    ]
    selected.sort(key=lambda asset: asset["id"])
    start = (page - 1) * size
    page_assets = selected[start : start + size]
    items = [_response_asset(asset) for asset in page_assets]
    next_page = str(page + 1) if start + size < len(selected) else None
    return {
        "assets": {
            "items": items,
            "count": len(items),
            "total": len(selected),
            "nextPage": next_page,
        }
    }


def archive_original(scenario: dict[str, Any], path: str) -> bytes | None:
    """Resolve one exact synthetic original without exposing filesystem data."""
    parts = path.split("/")
    if len(parts) != 5 or parts[:3] != ["", "api", "assets"] or parts[4] != "original":
        return None
    asset_id = parts[3]
    for asset in _assets(scenario):
        if asset["id"] == asset_id:
            download_body = asset.get("download_body", asset["body"])
            return download_body if isinstance(download_body, bytes) else None
    return None


def archive_asset_response(
    scenario: dict[str, Any], path: str
) -> dict[str, object] | None:
    """Resolve one related synthetic asset for read-only inventory expansion."""
    parts = path.split("/")
    if len(parts) != 4 or parts[:3] != ["", "api", "assets"]:
        return None
    for asset in _assets(scenario):
        if asset["id"] == parts[3]:
            return _response_asset(asset)
    return None


def _assets(scenario: dict[str, Any]) -> list[dict[str, Any]]:
    assets = scenario.get("archive_assets", [])
    if not isinstance(assets, list):
        return []
    validated = []
    for asset in assets:
        if (
            isinstance(asset, dict)
            and isinstance(asset.get("id"), str)
            and isinstance(asset.get("filename"), str)
            and isinstance(asset.get("body"), bytes)
            and asset.get("type") in {"IMAGE", "VIDEO"}
            and asset.get("visibility") in {"timeline", "archive", "hidden", "linked"}
            and isinstance(asset.get("trashed", False), bool)
        ):
            validated.append(asset)
    return validated


def _response_asset(asset: dict[str, Any]) -> dict[str, object]:
    body = asset["body"]
    response = {
        "id": asset["id"],
        "originalFileName": asset["filename"],
        "checksum": base64.b64encode(hashlib.sha1(body).digest()).decode("ascii"),
        "type": asset["type"],
        "exifInfo": {"fileSizeInByte": len(body)},
    }
    related = asset.get("live_photo_video_id")
    if isinstance(related, str):
        response["livePhotoVideoId"] = related
    return response
