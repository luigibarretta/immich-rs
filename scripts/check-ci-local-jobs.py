#!/usr/bin/env python3
"""Assert that every ci-local manifest step runs in exactly one CI job.

The local gate (``ci-local.json``) and ``.gitea/workflows/ci.yml`` cannot drift
apart: a job owns a step when it calls ``ci-local.mjs --ci [--no-needs] --only
a,b``; steps pulled in only through ``needs`` are not assignments. The checker
also fails when a ``--no-needs`` call relies on a step no earlier call in the
job ran, when the workflow repeats a manifest command outside ci-local, when
the audit step stops auditing Cargo.lock with ``--deny warnings`` or its
ignores drift from ``deny.toml``, and when the workflow checks out repo-tooling
at a commit other than ``dependencies/repo-tooling.sha``.
"""

from __future__ import annotations

from dataclasses import dataclass, field
import json
from pathlib import Path
import re
import sys
import tomllib

ROOT = Path(__file__).resolve().parent.parent
WORKFLOW = ".gitea/workflows/ci.yml"
AUDIT_STEP = "audit-lockfiles"
# Shorter commands (for example ``bash x.sh``) are too generic to flag.
MIN_DUPLICATE_COMMAND = 16
JOB_HEADER = re.compile(r"^ {2}([A-Za-z0-9_-]+):\s*$")
TOOLING_REF = re.compile(r"^\s+ref:\s*([0-9a-f]{40})\s*$")


@dataclass(frozen=True)
class Step:
    """The parts of a manifest step the ownership check needs."""

    name: str
    command: str | None = None
    uses: str | None = None
    needs: tuple[str, ...] = ()
    config: dict = field(default_factory=dict)


@dataclass(frozen=True)
class Call:
    """One ``ci-local.mjs --ci --only`` invocation inside a workflow job."""

    job: str
    only: tuple[str, ...]
    no_needs: bool


def manifest_steps(manifest: dict) -> list[Step]:
    """Read the steps of a ci-local manifest."""
    steps = []
    for raw in manifest["steps"]:
        run = raw.get("run")
        command = " ".join(run) if isinstance(run, list) else run
        steps.append(
            Step(
                raw["name"],
                command,
                raw.get("uses"),
                tuple(raw.get("needs", ())),
                raw.get("with", {}),
            )
        )
    return steps


def parse_call(job: str | None, args: str, line: str) -> Call | str:
    """Parse the options after ``ci-local.mjs``; return a problem string on error."""
    tokens = iter(args.split())
    ci = no_needs = False
    only = None
    for token in tokens:
        if token == "--ci":
            ci = True
        elif token == "--no-needs":
            no_needs = True
        elif token == "--only":
            only = next(tokens, None)
        elif not token.startswith("--"):
            break
    if job is None:
        return f"ci-local call outside a job: {line}"
    if not ci:
        return f"job {job}: ci-local call without --ci: {line}"
    if not only:
        return f"job {job}: ci-local call without --only: {line}"
    return Call(job, tuple(only.split(",")), no_needs)


def workflow_calls(workflow: str) -> tuple[list[Call], list[str]]:
    """Return the ci-local calls in workflow order plus malformed-call problems."""
    calls: list[Call] = []
    problems: list[str] = []
    in_jobs = False
    job = None
    for line in workflow.splitlines():
        if line.rstrip() == "jobs:":
            in_jobs = True
        header = JOB_HEADER.match(line) if in_jobs else None
        if header:
            job = header.group(1)
        if line.lstrip().startswith("#") or "ci-local.mjs" not in line:
            continue
        parsed = parse_call(job, line.split("ci-local.mjs", 1)[1], line.strip())
        (calls if isinstance(parsed, Call) else problems).append(parsed)
    return calls, problems


def closure(names: tuple[str, ...], by_name: dict[str, Step]) -> set[str]:
    """Return the named steps plus everything they transitively need."""
    seen: set[str] = set()
    queue = list(names)
    while queue:
        name = queue.pop()
        if name in by_name and name not in seen:
            seen.add(name)
            queue.extend(by_name[name].needs)
    return seen


def missing_needs(call: Call, by_name: dict[str, Step], ready: set[str]) -> list[str]:
    """Report needs of a ``--no-needs`` call that no earlier call in the job ran."""
    return [
        f"job {call.job}: --no-needs step {name} needs {need}, "
        "which no earlier ci-local call in the job ran"
        for name in call.only
        if name in by_name
        for need in by_name[name].needs
        if need not in ready and need not in call.only
    ]


def duplicated_commands(steps: list[Step], workflow: str) -> list[str]:
    """CI must run manifest commands through ci-local, not repeat them in YAML."""
    yaml = "\n".join(line for line in workflow.splitlines() if "ci-local.mjs" not in line)
    return [
        f'workflow repeats the command of step {step.name} outside ci-local: "{step.command}"'
        for step in steps
        if step.command and len(step.command) >= MIN_DUPLICATE_COMMAND and step.command in yaml
    ]


def check_audit_step(audit: Step | None, reviewed: list[str]) -> list[str]:
    """Keep the audit step on Cargo.lock, warnings denied, with the deny.toml ignores.

    ``cargo deny check`` and ``cargo audit`` read different ignore lists; both
    must accept exactly the same reviewed advisories.
    """
    if audit is None:
        return [f"step {AUDIT_STEP} is missing from the manifest"]
    if audit.uses != "audit-all-lockfiles":
        return [f"step {AUDIT_STEP} must use audit-all-lockfiles"]
    if "cargo" not in audit.config.get("ecosystems", ["cargo", "npm"]):
        return [f"step {AUDIT_STEP} does not audit cargo"]
    cargo = audit.config.get("cargo", {})
    problems = []
    args = cargo.get("args", [])
    if not any(args[index : index + 2] == ["--deny", "warnings"] for index in range(len(args))):
        problems.append(f"step {AUDIT_STEP} must pass --deny warnings to cargo audit")
    ignored = sorted(entry["id"] for entry in cargo.get("ignore", []))
    if ignored != sorted(reviewed):
        problems.append(
            f"step {AUDIT_STEP} ignores {ignored} but deny.toml reviews {sorted(reviewed)}"
        )
    return problems


def check_tooling_ref(workflow: str, pin: str) -> list[str]:
    """The workflow's repo-tooling checkout must use the pinned commit."""
    refs = {match.group(1) for line in workflow.splitlines() if (match := TOOLING_REF.match(line))}
    if refs != {pin}:
        return [f"workflow checks out repo-tooling at {sorted(refs)}, pin is {pin}"]
    return []


def check_assignments(steps: list[Step], workflow: str, reviewed: list[str]) -> list[str]:
    """Return every ownership problem; an empty list means the gate is aligned."""
    by_name = {step.name: step for step in steps}
    calls, problems = workflow_calls(workflow)
    owners: dict[str, list[str]] = {}
    available: dict[str, set[str]] = {}
    for call in calls:
        for name in call.only:
            if name in by_name:
                owners.setdefault(name, []).append(call.job)
            else:
                problems.append(f"job {call.job}: unknown ci-local step {name}")
        ready = available.setdefault(call.job, set())
        if call.no_needs:
            problems.extend(missing_needs(call, by_name, ready))
            ready.update(call.only)
        else:
            ready.update(closure(call.only, by_name))
    for step in steps:
        jobs = owners.get(step.name, [])
        if not jobs:
            problems.append(f"step {step.name} runs in no CI job")
        elif len(jobs) > 1:
            problems.append(
                f"step {step.name} runs in {len(jobs)} CI assignments: {', '.join(jobs)}"
            )
    problems.extend(duplicated_commands(steps, workflow))
    problems.extend(check_audit_step(by_name.get(AUDIT_STEP), reviewed))
    return problems


def main() -> int:
    """Check the repository's manifest against its Gitea workflow."""
    manifest = json.loads((ROOT / "ci-local.json").read_text(encoding="utf-8"))
    deny = tomllib.loads((ROOT / "deny.toml").read_text(encoding="utf-8"))
    pin = (ROOT / "dependencies" / "repo-tooling.sha").read_text(encoding="utf-8").strip()
    steps = manifest_steps(manifest)
    workflow = (ROOT / WORKFLOW).read_text(encoding="utf-8")
    reviewed = [
        entry if isinstance(entry, str) else entry["id"]
        for entry in deny["advisories"].get("ignore", [])
    ]
    problems = check_assignments(steps, workflow, reviewed)
    problems.extend(check_tooling_ref(workflow, pin))
    for problem in problems:
        print(f"ci-local jobs: {problem}", file=sys.stderr)
    if not problems:
        print(f"ci-local jobs: all {len(steps)} manifest steps run in exactly one CI job")
    return 1 if problems else 0


if __name__ == "__main__":
    raise SystemExit(main())
