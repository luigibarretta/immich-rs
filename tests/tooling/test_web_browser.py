import pathlib
import subprocess
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "test-web-browser.sh"


class WebBrowserGateTests(unittest.TestCase):
    def test_gate_is_syntax_checked_loopback_only_and_self_cleaning(self) -> None:
        subprocess.run(["bash", "-n", str(SCRIPT)], check=True)
        text = SCRIPT.read_text(encoding="utf-8")
        for required in (
            "mktemp -d",
            "trap cleanup EXIT INT TERM",
            "127.0.0.1",
            "--headless=new",
            "--disable-background-networking",
            "--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE 127.0.0.1",
            "--dump-dom",
            "--screenshot=",
            "synthetic-browser-bootstrap",
            "rm -rf -- \"$workspace\"",
        ):
            self.assertIn(required, text)
        for forbidden in ("authentik", "immich-go", "production", "0.0.0.0"):
            self.assertNotIn(forbidden, text.lower())


if __name__ == "__main__":
    unittest.main()
