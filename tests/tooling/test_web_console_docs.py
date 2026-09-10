import pathlib
import re
import tomllib
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[2]


class WebConsoleDocumentationTests(unittest.TestCase):
    def test_operations_and_metrics_contract_remain_explicit(self) -> None:
        operations = (ROOT / "docs" / "web-console-operations.md").read_text(
            encoding="utf-8"
        )
        resources = (ROOT / "docs" / "web-console-resources.md").read_text(
            encoding="utf-8"
        )
        metric_source = (ROOT / "crates" / "immich-web" / "src" / "metrics.rs").read_text(
            encoding="utf-8"
        )
        configuration = (ROOT / "docs" / "configuration.md").read_text(
            encoding="utf-8"
        )
        migration = (ROOT / "docs" / "migration-from-immich-go.md").read_text(
            encoding="utf-8"
        )
        for required in (
            "Do not copy a live SQLite database",
            "complete configured state root",
            "Sessions and grants are intentionally absent",
            "fresh offline",
            "no in-console garbage",
            "Never combine artifacts",
            "unattended scraping is supported",
            "127.0.0.1",
        ):
            self.assertIn(required, operations)
        metrics = {
            "sessions_active",
            "jobs_queued",
            "jobs_running",
            "jobs_completed",
            "jobs_failed",
            "jobs_cancelled",
            "jobs_retained",
            "sse_subscribers",
            "history_rows",
        }
        for metric in metrics:
            self.assertIn(f"`immich_rs_web_{metric}`", resources)
        implementation_names = set(re.findall(r"immich_rs_web_[a-z_]+", metric_source))
        self.assertEqual(implementation_names, {f"immich_rs_web_{name}" for name in metrics})
        self.assertIn("no labels", resources)
        normalized_resources = " ".join(resources.lower().split())
        self.assertIn("no separate metrics listener", normalized_resources)
        self.assertIn("immediate tcp peer cidr", normalized_resources)
        self.assertIn("query-string credential", normalized_resources)
        self.assertIn("IMMICH_RS_WEB_CONFIG", configuration)
        self.assertIn("production_read", configuration)
        web_section = configuration.split("## Web Console configuration namespace", 1)[1]
        sample = re.search(r"```toml\n(.*?)```", web_section, re.DOTALL)
        self.assertIsNotNone(sample)
        parsed = tomllib.loads(sample.group(1))
        self.assertEqual(parsed["schema_version"], 1)
        self.assertEqual(parsed["web"]["listen_address"], "127.0.0.1:2285")
        self.assertIn("exact single-use", migration)
        self.assertIn("Immich-to-Immich production cutover is intentionally absent", migration)


if __name__ == "__main__":
    unittest.main()
