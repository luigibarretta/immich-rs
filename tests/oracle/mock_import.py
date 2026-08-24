"""Synthetic metadata and album behavior for the bounded Immich mock."""

from __future__ import annotations

from threading import Lock
from typing import Any
from urllib.parse import parse_qsl


class MockImportState:
    """Thread-safe observable import state containing only synthetic values."""

    def __init__(self, scenario: dict[str, Any] | None = None) -> None:
        self._metadata: dict[str, dict[str, Any]] = {}
        self._albums: dict[str, dict[str, Any]] = {}
        source_albums = scenario.get("archive_albums", []) if isinstance(scenario, dict) else []
        self._source_albums = [dict(album) for album in source_albums if isinstance(album, dict)]
        self._next_album = 100
        self._lock = Lock()

    def update_metadata(self, asset_id: str, body: Any) -> tuple[int, object, bool]:
        if not isinstance(body, dict) or not body:
            return 400, {"message": "invalid synthetic metadata"}, False
        allowed = {"dateTimeOriginal", "description", "latitude", "longitude"}
        if not set(body).issubset(allowed) or ("latitude" in body) != ("longitude" in body):
            return 400, {"message": "invalid synthetic metadata fields"}, False
        with self._lock:
            self._metadata[asset_id] = dict(body)
        return 200, {"id": asset_id}, True

    def list_albums(self, query: str) -> tuple[int, object, bool]:
        values = dict(parse_qsl(query, keep_blank_values=True))
        name = values.get("name")
        if values.get("isOwned") != "true":
            return 400, {"message": "invalid synthetic album query"}, False
        if name is None:
            albums = [
                {
                    "id": album.get("id"),
                    "albumName": album.get("name"),
                    "assetCount": len(album.get("asset_ids", [])),
                }
                for album in self._source_albums
            ]
            return 200, albums, False
        if not isinstance(name, str):
            return 400, {"message": "invalid synthetic album query"}, False
        with self._lock:
            albums = [self._album_response(album) for album in self._albums.values() if album["name"] == name]
        return 200, albums, False

    def create_album(self, body: Any) -> tuple[int, object, bool]:
        name = body.get("albumName") if isinstance(body, dict) else None
        if not isinstance(name, str) or not name or len(name.encode()) > 4_096:
            return 400, {"message": "invalid synthetic album"}, False
        with self._lock:
            self._next_album += 1
            album_id = f"00000000-0000-4000-8000-{self._next_album:012d}"
            album = {"id": album_id, "name": name, "assets": set()}
            self._albums[album_id] = album
            response = self._album_response(album)
        return 201, response, True

    def add_members(self, album_id: str, body: Any) -> tuple[int, object, bool]:
        ids = body.get("ids") if isinstance(body, dict) else None
        if not isinstance(ids, list) or not ids or any(not isinstance(asset_id, str) for asset_id in ids):
            return 400, {"message": "invalid synthetic membership"}, False
        with self._lock:
            album = self._albums.get(album_id)
            if album is None:
                return 404, {"message": "synthetic album missing"}, False
            results = []
            for asset_id in ids:
                duplicate = asset_id in album["assets"]
                album["assets"].add(asset_id)
                result: dict[str, Any] = {"id": asset_id, "success": not duplicate}
                if duplicate:
                    result["error"] = "duplicate"
                results.append(result)
        return 200, results, True

    def snapshot(self) -> dict[str, Any]:
        with self._lock:
            return {
                "metadata_count": len(self._metadata),
                "metadata": {key: dict(value) for key, value in sorted(self._metadata.items())},
                "album_count": len(self._albums),
                "album_memberships": sum(len(album["assets"]) for album in self._albums.values()),
            }

    @staticmethod
    def _album_response(album: dict[str, Any]) -> dict[str, str]:
        return {"id": album["id"], "albumName": album["name"]}


def route_import(
    state: MockImportState,
    method: str,
    path: str,
    query: str,
    body: Any,
) -> tuple[int, object, bool] | None:
    """Route only the import API subset; return None for unrelated endpoints."""
    if method == "PUT" and path.startswith("/api/assets/") and path.count("/") == 3:
        return state.update_metadata(path.rsplit("/", 1)[1], body)
    if method == "GET" and path == "/api/albums":
        return state.list_albums(query)
    if method == "POST" and path == "/api/albums":
        return state.create_album(body)
    if method == "PUT" and path.startswith("/api/albums/") and path.endswith("/assets"):
        parts = path.split("/")
        if len(parts) == 5:
            return state.add_members(parts[3], body)
    return None
