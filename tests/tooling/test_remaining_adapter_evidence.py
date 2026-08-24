"""Tests for fail-closed validation of the remaining adapter evidence."""

import importlib.util
import json
from pathlib import Path
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
REPORTS = {
    "apple_gate": ROOT / "docs/evidence/phase9-source-import-2026-08-24.json",
    "picasa_gate": ROOT / "docs/evidence/phase10-source-import-2026-08-24.json",
    "apple_benchmark": ROOT / "benchmarks/evidence/phase9-source-import-benchmark-2026-08-24.json",
    "picasa_benchmark": ROOT / "benchmarks/evidence/phase10-source-import-benchmark-2026-08-24.json",
    "migration_gate": ROOT / "docs/evidence/phase11-disposable-migration-2026-08-24.json",
    "migration_benchmark": ROOT / "benchmarks/evidence/phase11-real-2026-08-24.json",
}


def load_checker(name: str):
    path = ROOT / "scripts" / name
    module_name = f"tested_{name.removesuffix('.py').replace('-', '_')}"
    spec = importlib.util.spec_from_file_location(module_name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


source_gate = load_checker("check-source-import-evidence.py")
source_benchmark = load_checker("check-source-import-benchmark.py")
migration_gate = load_checker("check-migration-evidence.py")
migration_benchmark = load_checker("check-real-migration-benchmark.py")


class RemainingAdapterEvidenceTests(unittest.TestCase):
    def tampered(self, source: Path, change) -> Path:
        report = json.loads(source.read_text(encoding="utf-8"))
        change(report)
        temporary = tempfile.NamedTemporaryFile(
            mode="w", encoding="utf-8", suffix=".json", delete=False
        )
        with temporary:
            json.dump(report, temporary)
        self.addCleanup(Path(temporary.name).unlink, missing_ok=True)
        return Path(temporary.name)

    def test_committed_reports_are_self_consistent(self) -> None:
        source_gate.validate(REPORTS["apple_gate"])
        source_gate.validate(REPORTS["picasa_gate"])
        source_benchmark.validate(REPORTS["apple_benchmark"])
        source_benchmark.validate(REPORTS["picasa_benchmark"])
        migration_gate.validate(REPORTS["migration_gate"])
        migration_benchmark.validate(REPORTS["migration_benchmark"])

    def test_source_cleanup_tampering_fails_closed(self) -> None:
        path = self.tampered(
            REPORTS["apple_gate"],
            lambda report: report["cleanup"].update({"credentials_removed": False}),
        )
        with self.assertRaises(source_gate.EvidenceError):
            source_gate.validate(path)

    def test_source_aggregate_tampering_fails_closed(self) -> None:
        path = self.tampered(
            REPORTS["picasa_benchmark"],
            lambda report: report["aggregate"]["immich_rs"]["wall_time_seconds"].update(
                {"median": 0}
            ),
        )
        with self.assertRaises(source_benchmark.EvidenceError):
            source_benchmark.validate(path)

    def test_source_immutability_tampering_fails_closed(self) -> None:
        path = self.tampered(
            REPORTS["migration_gate"],
            lambda report: report["plan"].update(
                {"source_byte_identical_after_apply": False}
            ),
        )
        with self.assertRaises(migration_gate.EvidenceError):
            migration_gate.validate(path)

    def test_migration_operation_tampering_fails_closed(self) -> None:
        path = self.tampered(
            REPORTS["migration_benchmark"],
            lambda report: report["raw_samples"][0]["immich_rs"]["operations"].update(
                {"source_assets": 9}
            ),
        )
        with self.assertRaises(migration_benchmark.EvidenceError):
            migration_benchmark.validate(path)


if __name__ == "__main__":
    unittest.main()
