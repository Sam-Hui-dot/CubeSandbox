# Copyright (c) 2026 Tencent Inc.
# SPDX-License-Identifier: Apache-2.0

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from env_utils import opencode_command


class CommandTest(unittest.TestCase):
    def test_opencode_command_uses_headless_run_mode(self) -> None:
        command = opencode_command(
            "fix tests",
            workspace="/workspace",
            model="openai/gpt-4.1-mini",
            title="cube-demo",
        )
        self.assertIn("opencode run", command)
        self.assertIn("--model openai/gpt-4.1-mini", command)
        self.assertIn("--dir /workspace", command)
        self.assertIn("--auto", command)
        self.assertIn("--format json", command)

    def test_prompt_is_shell_quoted(self) -> None:
        prompt = 'write file"; touch /tmp/pwned; echo "'
        command = opencode_command(
            prompt,
            workspace="/workspace",
            model="openai/gpt-4.1-mini",
        )
        self.assertIn("'write file", command)
        self.assertIn("touch /tmp/pwned", command)
        self.assertTrue(command.endswith("'"))

    def test_continue_last_switch(self) -> None:
        command = opencode_command(
            "continue",
            workspace="/workspace",
            model="openai/gpt-4.1-mini",
            continue_last=True,
        )
        self.assertIn("--continue", command)


if __name__ == "__main__":
    unittest.main()
