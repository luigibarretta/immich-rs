"""Tests for the comparable Phase-2 benchmark corpus view."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import unittest


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPOSITORY_ROOT / "scripts" / "prepare-phase2-benchmark-corpus.sh"


class Phase2BenchmarkCorpusTests(unittest.TestCase):
    def test_live_pair_is_renamed_to_two_standalone_assets_idempotently(self) -> None:
        with tempfile.TemporaryDirectory(prefix="immich-rs-disposable.corpus-test.") as temporary:
            workspace = Path(temporary)
            source = workspace / "source"
            output = workspace / "output"
            source.mkdir()
            roles = {
                "clip.mp4": "standalone-video",
                "image.jpg": "standalone-image",
                "image.xmp": "xmp-sidecar",
                "live.jpg": "live-photo-image",
                "live.mov": "live-photo-video",
            }
            files = []
            for path, role in roles.items():
                content = f"synthetic {path}\n".encode()
                (source / path).write_bytes(content)
                files.append(
                    {"path": path, "role": role, "bytes": len(content), "sha256": hashlib.sha256(content).hexdigest()}
                )
            manifest = workspace / "source-manifest.json"
            manifest.write_text(
                json.dumps(
                    {
                        "schema": "phase2-corpus-v1",
                        "fixture_id": "synthetic-phase2-media-matrix",
                        "synthetic": True,
                        "license": "CC0-1.0",
                        "generator": {"recipe": "ffmpeg-lavfi-apple-live-v2"},
                        "files": files,
                        "expected": {"upload_operations": 4, "xmp_sidecars": 1, "live_photo_pairs": 1},
                    }
                ),
                encoding="utf-8",
            )
            output_manifest = workspace / "output-manifest.json"
            command = [
                str(SCRIPT), "--source", str(source), "--source-manifest", str(manifest),
                "--output", str(output), "--output-manifest", str(output_manifest),
            ]
            subprocess.run(command, check=True)
            first = output_manifest.read_bytes()
            subprocess.run(command, check=True)
            self.assertEqual(output_manifest.read_bytes(), first)
            derived = json.loads(first)
            self.assertEqual(derived["expected"]["live_photo_pairs"], 0)
            self.assertEqual(derived["expected"]["visible_assets"], 4)
            self.assertEqual(sorted(path.name for path in output.iterdir()), [
                "clip.mp4", "image.jpg", "image.xmp", "motion.mov", "still.jpg"
            ])


if __name__ == "__main__":
    unittest.main()
