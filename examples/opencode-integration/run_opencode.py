#!/usr/bin/env python3
# Copyright (c) 2026 Tencent Inc.
# SPDX-License-Identifier: Apache-2.0

from __future__ import annotations

import argparse
import os
import shlex
import sys

from _opencode_common import ensure_success, run_command, sandbox_identifier
from env_utils import (
    build_opencode_env,
    int_env,
    load_local_dotenv,
    opencode_command,
    opencode_config_json,
    opencode_model,
    opencode_provider,
    opencode_workspace,
    require_provider_key,
    required,
    setup_dev_sidecar_if_requested,
    shell_join,
)

DEFAULT_PROMPT_TEMPLATE = (
    "You are in {workspace}. Implement the missing add(a, b) function in "
    "calculator.py, run `python3 -m unittest discover -v`, and write "
    "{workspace}/result.md containing the exact marker OPENCODE_CUBE_OK plus "
    "one short sentence about the test result."
)


def default_prompt(workspace: str) -> str:
    return DEFAULT_PROMPT_TEMPLATE.format(workspace=workspace)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run a one-shot OpenCode coding-agent task inside CubeSandbox."
    )
    parser.add_argument("--template", default=os.environ.get("CUBE_TEMPLATE_ID"))
    parser.add_argument("--prompt", default=None)
    parser.add_argument("--workspace", default=opencode_workspace())
    parser.add_argument("--title", default="cube-opencode-demo")
    parser.add_argument(
        "--sandbox-timeout",
        type=int,
        default=int_env("OPENCODE_SANDBOX_TIMEOUT", 1800),
    )
    parser.add_argument(
        "--exec-timeout",
        type=int,
        default=int_env("OPENCODE_EXEC_TIMEOUT", 900),
    )
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--keep-alive", action="store_true")
    parser.add_argument("--raw", action="store_true", help="Reserved for symmetry.")
    args = parser.parse_args()
    if args.prompt is None:
        args.prompt = default_prompt(args.workspace)
    return args


def seed_project(sandbox, workspace: str, timeout: int) -> None:
    quoted_workspace = shlex.quote(workspace)
    config_json = opencode_config_json()
    command = f"""mkdir -p {quoted_workspace}
cat > {quoted_workspace}/opencode.json <<'EOF'
{config_json}
EOF
cat > {quoted_workspace}/calculator.py <<'EOF'
def add(a: int, b: int) -> int:
    raise NotImplementedError("OpenCode should implement this")


if __name__ == "__main__":
    print(add(2, 3))
EOF
cat > {quoted_workspace}/test_calculator.py <<'EOF'
import unittest

from calculator import add


class CalculatorTest(unittest.TestCase):
    def test_adds_two_numbers(self) -> None:
        self.assertEqual(add(2, 3), 5)


if __name__ == "__main__":
    unittest.main()
EOF
cat > {quoted_workspace}/README.md <<'EOF'
# CubeSandbox OpenCode Smoke Project

Implement calculator.add, run the test, and summarize the result.
EOF
"""
    result = run_command(sandbox, command, timeout=timeout)
    ensure_success(result, "seed OpenCode workspace")


def verify_workspace(sandbox, workspace: str, timeout: int) -> None:
    quoted_workspace = shlex.quote(workspace)
    command = shell_join(
        f"cd {quoted_workspace}",
        "python3 -m unittest discover -v",
        "test -f result.md",
        "grep -q OPENCODE_CUBE_OK result.md",
        "printf '\\n--- result.md ---\\n'",
        "cat result.md",
    )
    result = run_command(sandbox, command, timeout=timeout)
    ensure_success(result, "verify OpenCode output")
    if getattr(result, "stdout", ""):
        print(result.stdout)


def main() -> int:
    load_local_dotenv()
    args = parse_args()

    template_id = args.template or required("CUBE_TEMPLATE_ID")
    required("E2B_API_URL")
    required("E2B_API_KEY")
    model = opencode_model()
    provider = opencode_provider(model)
    require_provider_key(provider)

    command = opencode_command(
        args.prompt,
        workspace=args.workspace,
        model=model,
        title=args.title,
    )
    envs = build_opencode_env(include_secrets=True)

    if args.dry_run:
        print(f"Template: {template_id}")
        print(f"Provider: {provider}")
        print(f"Model: {model}")
        print(f"Workspace: {args.workspace}")
        print(f"Command: {command}")
        print("Secrets: redacted")
        return 0

    setup_dev_sidecar_if_requested()
    from e2b import Sandbox

    print(f"Creating sandbox from template: {template_id}")
    sandbox = None
    result = None
    try:
        sandbox = Sandbox.create(template=template_id, timeout=args.sandbox_timeout)
        sandbox_id = sandbox_identifier(sandbox)
        print(f"Sandbox ready: {sandbox_id}")

        version_result = run_command(sandbox, "opencode --version", timeout=60)
        ensure_success(version_result, "check OpenCode version")
        print(f"OpenCode version: {getattr(version_result, 'stdout', '').strip()}")

        seed_project(sandbox, args.workspace, timeout=60)
        print(f"Seeded deterministic project in {args.workspace}")

        print("\nRunning OpenCode task...\n")
        result = run_command(
            sandbox,
            command,
            cwd=args.workspace,
            envs=envs,
            timeout=args.exec_timeout,
            stream=True,
        )
        ensure_success(result, "run OpenCode")

        print("\nVerifying generated project state...")
        verify_workspace(sandbox, args.workspace, timeout=120)
        return 0
    finally:
        if sandbox is not None and not args.keep_alive:
            try:
                sandbox.kill()
                print("\nSandbox killed.")
            except Exception as exc:  # noqa: BLE001
                print(f"Warning: failed to kill sandbox: {exc}", file=sys.stderr)
        elif sandbox is not None:
            print(f"\nSandbox kept alive: {sandbox_identifier(sandbox)}")


if __name__ == "__main__":
    sys.exit(main())
