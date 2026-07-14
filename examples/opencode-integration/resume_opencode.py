#!/usr/bin/env python3
# Copyright (c) 2026 Tencent Inc.
# SPDX-License-Identifier: Apache-2.0

"""Demonstrate OpenCode state persistence across CubeSandbox pause/resume."""

from __future__ import annotations

import argparse
import os
import shlex
import sys

from e2b import Sandbox

from _opencode_common import (
    ensure_success,
    pause_sandbox,
    run_command,
    sandbox_identifier,
    warn_direct_secret_env,
)
from env_utils import (
    build_opencode_env,
    int_env,
    load_local_dotenv,
    opencode_command,
    opencode_config_json,
    opencode_model,
    opencode_provider,
    opencode_workspace,
    provider_key_name,
    require_provider_key,
    required,
    setup_dev_sidecar_if_requested,
    shell_join,
)

TURN_1_PROMPT = (
    "In {workspace}, create plan.md with a numbered two-step plan for adding a "
    "multiply function to calculator.py. Only write plan.md."
)

TURN_2_PROMPT = (
    "Continue from the previous session. Read plan.md, add multiply(a, b) to "
    "calculator.py, add a unittest for multiply(4, 5) == 20, run "
    "`python3 -m unittest discover -v`, and write progress.md with the exact "
    "marker OPENCODE_RESUME_OK."
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run OpenCode before and after CubeSandbox pause/resume."
    )
    parser.add_argument("--template", default=os.environ.get("CUBE_TEMPLATE_ID"))
    parser.add_argument("--workspace", default=opencode_workspace())
    parser.add_argument("--title", default="cube-opencode-resume")
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
    return parser.parse_args()


def seed_project(sandbox: Sandbox, workspace: str, timeout: int) -> None:
    quoted_workspace = shlex.quote(workspace)
    command = f"""mkdir -p {quoted_workspace}
cat > {quoted_workspace}/opencode.json <<'EOF'
{opencode_config_json()}
EOF
cat > {quoted_workspace}/calculator.py <<'EOF'
def add(a: int, b: int) -> int:
    return a + b
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
"""
    result = run_command(sandbox, command, timeout=timeout)
    ensure_success(result, "seed resume workspace")


def run_turn(
    sandbox: Sandbox,
    workspace: str,
    prompt: str,
    title: str,
    model: str,
    envs: dict[str, str],
    timeout: int,
    *,
    continue_last: bool = False,
):
    command = opencode_command(
        prompt,
        workspace=workspace,
        model=model,
        title=title,
        continue_last=continue_last,
    )
    return run_command(
        sandbox,
        command,
        cwd=workspace,
        envs=envs,
        timeout=timeout,
        stream=True,
    )


def verify_after_resume(sandbox: Sandbox, workspace: str) -> None:
    quoted_workspace = shlex.quote(workspace)
    command = shell_join(
        f"cd {quoted_workspace}",
        "test -f plan.md",
        "test -f calculator.py",
        "test -d /root/.local/share/opencode",
        "printf '\\n--- plan.md survived pause/resume ---\\n'",
        "cat plan.md",
    )
    result = run_command(sandbox, command, timeout=60)
    ensure_success(result, "verify OpenCode state survived pause/resume")
    if getattr(result, "stdout", ""):
        print(result.stdout)


def verify_final(sandbox: Sandbox, workspace: str) -> None:
    quoted_workspace = shlex.quote(workspace)
    command = shell_join(
        f"cd {quoted_workspace}",
        "python3 -m unittest discover -v",
        "grep -q 'def multiply' calculator.py",
        "test -f progress.md",
        "grep -q OPENCODE_RESUME_OK progress.md",
        "printf '\\n--- progress.md ---\\n'",
        "cat progress.md",
    )
    result = run_command(sandbox, command, timeout=120)
    ensure_success(result, "verify final resumed output")
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
    key_name = provider_key_name(provider)
    envs = build_opencode_env(include_secrets=True)

    setup_dev_sidecar_if_requested()
    warn_direct_secret_env(key_name)

    sandbox = None
    sandbox_id = "unknown"
    try:
        print(f"Creating sandbox from template: {template_id}")
        sandbox = Sandbox.create(template=template_id, timeout=args.sandbox_timeout)
        sandbox_id = sandbox_identifier(sandbox)
        print(f"Sandbox ready: {sandbox_id}")

        seed_project(sandbox, args.workspace, timeout=60)

        print("\n=== Turn 1: create plan.md ===\n")
        result_1 = run_turn(
            sandbox,
            args.workspace,
            TURN_1_PROMPT.format(workspace=args.workspace),
            args.title,
            model,
            envs,
            args.exec_timeout,
        )
        ensure_success(result_1, "run OpenCode turn 1")

        print(f"\nPausing sandbox {sandbox_id}...")
        sandbox_id = pause_sandbox(sandbox)
        print(f"Paused. Resume handle: {sandbox_id}")

        print(f"\nReconnecting to {sandbox_id}...")
        sandbox = Sandbox.connect(sandbox_id=sandbox_id)
        print("Reconnected after resume.")

        verify_after_resume(sandbox, args.workspace)

        print("\n=== Turn 2: continue after resume ===\n")
        result_2 = run_turn(
            sandbox,
            args.workspace,
            TURN_2_PROMPT,
            args.title,
            model,
            envs,
            args.exec_timeout,
            continue_last=True,
        )
        ensure_success(result_2, "run OpenCode turn 2")

        verify_final(sandbox, args.workspace)
        return 0
    finally:
        if sandbox is not None:
            try:
                sandbox.kill()
                print(f"\nSandbox {sandbox_id} killed.")
            except Exception as exc:  # noqa: BLE001
                print(
                    f"Warning: failed to kill sandbox {sandbox_id}: {exc}",
                    file=sys.stderr,
                )


if __name__ == "__main__":
    sys.exit(main())
