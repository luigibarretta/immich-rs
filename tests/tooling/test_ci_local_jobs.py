"""Unit tests for scripts/check-ci-local-jobs.py and the real gate alignment."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import sys
import unittest

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "check_ci_local_jobs", REPOSITORY_ROOT / "scripts" / "check-ci-local-jobs.py"
)
assert SPEC is not None and SPEC.loader is not None
jobs = importlib.util.module_from_spec(SPEC)
# The dataclasses resolve their module through sys.modules.
sys.modules[SPEC.name] = jobs
SPEC.loader.exec_module(jobs)

AUDIT = jobs.Step(
    jobs.AUDIT_STEP,
    uses="audit-all-lockfiles",
    config={
        "ecosystems": ["cargo"],
        "cargo": {"ignore": [{"id": "X-1", "reason": "r"}], "args": ["--deny", "warnings"]},
    },
)
AUDIT_CALL = f"jobs:\n  audit:\n      - run: node x/ci-local.mjs --ci --only {jobs.AUDIT_STEP}\n"


def step(name: str, *needs: str):
    return jobs.Step(name, f"echo {name}", needs=needs)


def check(steps: list, workflow_jobs: str) -> list[str]:
    return jobs.check_assignments([*steps, AUDIT], AUDIT_CALL + workflow_jobs, ["X-1"])


class CiLocalJobsTests(unittest.TestCase):
    def test_the_repository_gate_is_aligned(self) -> None:
        self.assertEqual(jobs.main(), 0)

    def test_every_step_owned_by_exactly_one_job_passes(self) -> None:
        workflow = "  a:\n    steps:\n      - run: node x/ci-local.mjs --ci --only one,two\n"
        self.assertEqual(check([step("one"), step("two")], workflow), [])

    def test_unassigned_duplicated_and_unknown_steps_fail(self) -> None:
        workflow = (
            "  a:\n      - run: node x/ci-local.mjs --ci --only one,ghost\n"
            "  b:\n      - run: node x/ci-local.mjs --ci --only one\n"
        )
        problems = check([step("one"), step("lost")], workflow)
        self.assertIn("job a: unknown ci-local step ghost", problems)
        self.assertIn("step one runs in 2 CI assignments: a, b", problems)
        self.assertIn("step lost runs in no CI job", problems)

    def test_a_step_pulled_in_only_through_needs_is_not_an_assignment(self) -> None:
        workflow = "  a:\n      - run: node x/ci-local.mjs --ci --only build\n"
        self.assertEqual(
            check([step("install"), step("build", "install")], workflow),
            ["step install runs in no CI job"],
        )

    def test_no_needs_requires_an_earlier_call_in_the_same_job(self) -> None:
        steps = [step("install"), step("build", "install"), step("e2e", "build")]
        ok = (
            "  a:\n      - run: node x/ci-local.mjs --ci --only install\n"
            "  b:\n      - run: node x/ci-local.mjs --ci --only build\n"
            "      - run: node x/ci-local.mjs --ci --no-needs --only e2e\n"
        )
        self.assertEqual(check(steps, ok), [])
        other_job = ok.replace("--only build\n", "--only build\n  c:\n")
        self.assertTrue(
            any("--no-needs step e2e needs build" in problem for problem in check(steps, other_job))
        )

    def test_workflow_calls_must_use_ci_and_only(self) -> None:
        _, problems = jobs.workflow_calls(
            "jobs:\n  a:\n      - run: node x/ci-local.mjs --quick\n"
            "      - run: node x/ci-local.mjs --only a\n"
        )
        self.assertEqual(len(problems), 2)
        self.assertEqual(
            jobs.workflow_calls("jobs:\n  a:\n      # run `node x/ci-local.mjs --quick`\n"),
            ([], []),
        )

    def test_the_workflow_must_not_repeat_a_manifest_command(self) -> None:
        clippy = jobs.Step("clippy", "cargo clippy --locked -- -D warnings")
        workflow = (
            "  a:\n      - run: node x/ci-local.mjs --ci --only clippy\n"
            "      - run: cargo clippy --locked -- -D warnings\n"
        )
        self.assertEqual(
            check([clippy], workflow),
            [
                "workflow repeats the command of step clippy outside ci-local: "
                '"cargo clippy --locked -- -D warnings"'
            ],
        )

    def test_the_audit_step_denies_warnings_with_exactly_the_reviewed_ignores(self) -> None:
        self.assertEqual(jobs.check_audit_step(AUDIT, ["X-1"]), [])
        self.assertEqual(
            jobs.check_audit_step(AUDIT, ["X-1", "X-2"]),
            ["step audit-lockfiles ignores ['X-1'] but deny.toml reviews ['X-1', 'X-2']"],
        )
        lenient = jobs.Step(
            jobs.AUDIT_STEP, uses="audit-all-lockfiles", config={"cargo": {"ignore": []}}
        )
        self.assertEqual(
            jobs.check_audit_step(lenient, []),
            ["step audit-lockfiles must pass --deny warnings to cargo audit"],
        )
        npm_only = jobs.Step(
            jobs.AUDIT_STEP, uses="audit-all-lockfiles", config={"ecosystems": ["npm"]}
        )
        self.assertEqual(
            jobs.check_audit_step(npm_only, []), ["step audit-lockfiles does not audit cargo"]
        )
        plain = jobs.Step(jobs.AUDIT_STEP, "cargo audit")
        self.assertEqual(
            jobs.check_audit_step(plain, []),
            ["step audit-lockfiles must use audit-all-lockfiles"],
        )
        self.assertEqual(
            jobs.check_audit_step(None, []), ["step audit-lockfiles is missing from the manifest"]
        )

    def test_the_repo_tooling_checkout_uses_the_pinned_commit(self) -> None:
        pin = "a" * 40
        workflow = f"      with:\n          ref: {pin}\n"
        self.assertEqual(jobs.check_tooling_ref(workflow, pin), [])
        self.assertEqual(
            jobs.check_tooling_ref(workflow, "b" * 40),
            [f"workflow checks out repo-tooling at ['{pin}'], pin is {'b' * 40}"],
        )
        self.assertTrue(jobs.check_tooling_ref("", pin))


if __name__ == "__main__":
    unittest.main()
