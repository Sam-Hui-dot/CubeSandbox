# Copyright (c) 2026 Tencent Inc.
# SPDX-License-Identifier: Apache-2.0

from __future__ import annotations

import os
import sys
import unittest
from dataclasses import dataclass
from io import StringIO
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))

from _opencode_common import ensure_success, redact_secrets, run_command, stream_writer


@dataclass
class FakeResult:
    stdout: str = ""
    stderr: str = ""
    exit_code: int = 0


class EnvOnlyCommands:
    def __init__(self) -> None:
        self.kwargs = {}

    def run(self, command: str, *, env: dict[str, str] | None = None):
        self.kwargs = {"command": command, "env": env}
        return FakeResult()


class FakeSandbox:
    def __init__(self) -> None:
        self.commands = EnvOnlyCommands()


class CommonHelperTest(unittest.TestCase):
    def test_run_command_uses_env_keyword_when_envs_is_not_supported(self) -> None:
        sandbox = FakeSandbox()
        result = run_command(sandbox, "true", envs={"OPENAI_API_KEY": "placeholder"})
        self.assertEqual(result.exit_code, 0)
        self.assertEqual(
            sandbox.commands.kwargs,
            {"command": "true", "env": {"OPENAI_API_KEY": "placeholder"}},
        )

    def test_redact_secrets_hides_known_env_values(self) -> None:
        with patch.dict(os.environ, {"OPENAI_API_KEY": "sk-secret-value"}, clear=True):
            self.assertEqual(
                redact_secrets("provider key is sk-secret-value"),
                "provider key is <redacted>",
            )

    def test_stream_writer_redacts_known_env_values(self) -> None:
        stream = StringIO()
        with patch.dict(os.environ, {"OPENAI_API_KEY": "sk-stream-secret"}, clear=True):
            stream_writer(stream)("stdout sk-stream-secret\n")
        self.assertEqual(stream.getvalue(), "stdout <redacted>\n")

    def test_redact_secrets_replaces_longest_values_first(self) -> None:
        env = {
            "A_API_KEY": "sk-foo",
            "B_API_KEY": "sk-foobar",
        }
        with patch.dict(os.environ, env, clear=True):
            self.assertEqual(redact_secrets("value sk-foobar"), "value <redacted>")

    def test_redact_secrets_uses_secret_name_suffixes(self) -> None:
        env = {
            "CSRF_TOKEN_EXPIRY_SECONDS": "3600",
            "SESSION_TOKEN": "secret-token",
        }
        with patch.dict(os.environ, env, clear=True):
            self.assertEqual(
                redact_secrets("ttl 3600 token secret-token"),
                "ttl 3600 token <redacted>",
            )

    def test_ensure_success_redacts_stdout_and_stderr(self) -> None:
        result = FakeResult(
            stdout="stdout sk-secret-value",
            stderr="stderr sk-secret-value",
            exit_code=1,
        )
        with patch.dict(os.environ, {"OPENAI_API_KEY": "sk-secret-value"}, clear=True):
            with self.assertRaises(SystemExit) as raised:
                ensure_success(result, "run command")
        message = str(raised.exception)
        self.assertIn("<redacted>", message)
        self.assertNotIn("sk-secret-value", message)


if __name__ == "__main__":
    unittest.main()
