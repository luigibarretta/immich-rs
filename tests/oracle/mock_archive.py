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
    album_ids = request.get("albumIds")
    include_trashed = request.get("withDeleted") is True
    trashed_only = isinstance(request.get("trashedAfter"), str)
    if (
        not isinstance(page, int)
        or page < 1
        or not isinstance(size, int)
        or not 1 <= size <= 1_000
        or visibility not in {"timeline", "archive", "hidden"}
        or (
            album_ids is not None
            and (
                not isinstance(album_ids, list)
                or not album_ids
                or any(not isinstance(album_id, str) for album_id in album_ids)
            )
        )
    ):
        return {"assets": {"items": [], "count": 0, "total": 0}}
    selected = [
        asset
        for asset in _assets(scenario)
        if asset["visibility"] == visibility
        and (
            (trashed_only and asset.get("trashed", False))
            or (not trashed_only and (include_trashed or not asset.get("trashed", False)))
        )
        and (
            album_ids is None
            or any(album_id in asset.get("album_ids", []) for album_id in album_ids)
        )
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
        "deviceAssetId": f"synthetic-{asset['id']}",
        "ownerId": "00000000-0000-4000-8000-000000000001",
        "deviceId": "synthetic-mock-device",
        "libraryId": None,
        "originalFileName": asset["filename"],
        "originalPath": f"/synthetic/{asset['filename']}",
        "originalMimeType": "image/jpeg" if asset["type"] == "IMAGE" else "video/quicktime",
        "checksum": base64.b64encode(hashlib.sha1(body).digest()).decode("ascii"),
        "type": asset["type"],
        "fileCreatedAt": asset.get("file_created_at", "2024-01-01T00:00:00Z"),
        "fileModifiedAt": asset.get("file_modified_at", "2024-01-01T00:00:01Z"),
        "localDateTime": asset.get("file_created_at", "2024-01-01T00:00:00Z"),
        "updatedAt": asset.get("file_modified_at", "2024-01-01T00:00:01Z"),
        "isFavorite": False,
        "isArchived": asset["visibility"] == "archive",
        "isTrashed": asset.get("trashed", False),
        "isOffline": False,
        "hasMetadata": True,
        "duration": "0:00:00.000000",
        "exifInfo": {"fileSizeInByte": len(body)},
    }
    exif = response["exifInfo"]
    if isinstance(exif, dict):
        for source, target in (
            ("date_time_original", "dateTimeOriginal"),
            ("description", "description"),
            ("latitude", "latitude"),
            ("longitude", "longitude"),
        ):
            value = asset.get(source)
            if value is not None:
                exif[target] = value
    related = asset.get("live_photo_video_id")
    if isinstance(related, str):
        response["livePhotoVideoId"] = related
    return response
