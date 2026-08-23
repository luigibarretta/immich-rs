from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
CHECKER = ROOT / "scripts/verify-takeout-postconditions.py"


def asset(identifier: str, name: str, description: str, instant: str, latitude=None, longitude=None):
    return {
        "id": identifier,
        "originalFileName": name,
        "exifInfo": {
            "description": description,
            "dateTimeOriginal": instant,
            "latitude": latitude,
            "longitude": longitude,
        },
    }


class Phase8PostconditionTests(unittest.TestCase):
    def responses(self, root: Path) -> list[Path]:
        search = {
            "assets": {
                "items": [
                    asset("asset-alpha", "alpha.png", "synthetic alpha description", "2024-01-01T00:00:00.000Z", 3.5, -4.25),
                    asset("asset-beta", "beta.png", "synthetic beta description", "2024-01-02T01:00:00+01:00"),
                    asset("asset-unicode", "café.png", "synthetic unicode description", "2024-01-03T00:00:00Z"),
                ]
            }
        }
        albums = [{"id": "album-id", "albumName": "Synthetic Album", "assetCount": 1}]
        album_assets = {"assets": {"items": [{"id": "asset-alpha"}]}}
        paths = [root / name for name in ("search.json", "albums.json", "album-assets.json")]
        for path, value in zip(paths, (search, albums, album_assets), strict=True):
            path.write_text(json.dumps(value), encoding="utf-8")
        return paths

    def invoke(self, paths: list[Path]) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [
                sys.executable,
                str(CHECKER),
                "--search",
                str(paths[0]),
                "--albums",
                str(paths[1]),
                "--album-assets",
                str(paths[2]),
            ],
            check=False,
            capture_output=True,
            text=True,
            timeout=10,
        )

    def test_exact_synthetic_postconditions_emit_only_aggregates(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            completed = self.invoke(self.responses(Path(temporary)))
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertEqual(
            json.loads(completed.stdout),
            {
                "schema": "phase8-takeout-postconditions-v1",
                "assets": 3,
                "metadata_assignments": 3,
                "albums": 1,
                "album_memberships": 1,
                "exact": True,
            },
        )
        self.assertNotIn("asset-alpha", completed.stdout)

    def test_wrong_album_member_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            paths = self.responses(Path(temporary))
            paths[2].write_text(
                json.dumps({"assets": {"items": [{"id": "asset-beta"}]}}),
                encoding="utf-8",
            )
            completed = self.invoke(paths)
        self.assertEqual(completed.returncode, 1)
        self.assertIn("membership drifted", completed.stderr)
        self.assertEqual(completed.stdout, "")


if __name__ == "__main__":
    unittest.main()
