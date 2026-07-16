# Copyright (c) 2026 Tencent Inc.
# SPDX-License-Identifier: Apache-2.0

from __future__ import annotations

import os
import sys
import unittest
from io import StringIO
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))

import env_utils


class EnvUtilsTest(unittest.TestCase):
    def test_model_must_include_provider_prefix(self) -> None:
        with patch.dict(os.environ, {"OPENCODE_MODEL": "gpt-4.1-mini"}, clear=True):
            with self.assertRaises(SystemExit):
                env_utils.opencode_model()

    def test_provider_is_model_prefix(self) -> None:
        with patch.dict(os.environ, {"OPENCODE_MODEL": "openai/gpt-4.1-mini"}, clear=True):
            self.assertEqual(env_utils.opencode_provider(), "openai")

    def test_provider_key_name_defaults_from_prefix(self) -> None:
        self.assertEqual(env_utils.provider_key_name("openai"), "OPENAI_API_KEY")
        self.assertEqual(env_utils.provider_key_name("deepseek"), "DEEPSEEK_API_KEY")
        self.assertEqual(env_utils.provider_key_name("custom"), "CUSTOM_API_KEY")

    def test_require_provider_key_fails_when_missing(self) -> None:
        with patch.dict(os.environ, {}, clear=True):
            with self.assertRaises(SystemExit):
                env_utils.require_provider_key("openai")

    def test_int_env_rejects_non_integer_values(self) -> None:
        with patch.dict(os.environ, {"OPENCODE_EXEC_TIMEOUT": "slow"}, clear=True):
            with self.assertRaises(SystemExit):
                env_utils.int_env("OPENCODE_EXEC_TIMEOUT", 900)

    def test_build_env_omits_all_api_keys_for_vault_mode(self) -> None:
        env = {
            "OPENCODE_MODEL": "openai/gpt-4.1-mini",
            "OPENAI_API_KEY": "sk-openai",
            "ANTHROPIC_API_KEY": "sk-anthropic",
            "HTTPS_PROXY": "http://proxy.local:8080",
        }
        with patch.dict(os.environ, env, clear=True):
            result = env_utils.build_opencode_env(include_secrets=False)
        self.assertNotIn("OPENAI_API_KEY", result)
        self.assertNotIn("ANTHROPIC_API_KEY", result)
        self.assertEqual(result["HTTPS_PROXY"], "http://proxy.local:8080")

    def test_build_env_includes_only_active_provider_key_by_default(self) -> None:
        env = {
            "OPENCODE_MODEL": "openai/gpt-4.1-mini",
            "OPENAI_API_KEY": "sk-openai",
            "ANTHROPIC_API_KEY": "sk-anthropic-must-not-leak",
        }
        with patch.dict(os.environ, env, clear=True):
            result = env_utils.build_opencode_env()
        self.assertEqual(result["OPENAI_API_KEY"], "sk-openai")
        self.assertNotIn("ANTHROPIC_API_KEY", result)

    def test_optional_preserves_empty_string(self) -> None:
        with patch.dict(os.environ, {"OPENCODE_DISABLE_AUTOUPDATE": ""}, clear=True):
            self.assertEqual(env_utils.optional("OPENCODE_DISABLE_AUTOUPDATE", "1"), "")

    def test_build_env_uses_official_autoupdate_variable_name(self) -> None:
        env = {
            "OPENCODE_MODEL": "openai/gpt-4.1-mini",
            "OPENAI_API_KEY": "sk-openai",
        }
        with patch.dict(os.environ, env, clear=True):
            result = env_utils.build_opencode_env()
        self.assertEqual(result["OPENCODE_DISABLE_AUTOUPDATE"], "1")
        self.assertNotIn("OPENCODE_DISABLE_AUTO_UPDATE", result)

    def test_llm_host_prefers_base_url(self) -> None:
        env = {
            "OPENCODE_MODEL": "openai/gpt-4.1-mini",
            "OPENCODE_BASE_URL": "https://llm.example.test/v1",
        }
        with patch.dict(os.environ, env, clear=True):
            self.assertEqual(env_utils.opencode_llm_host("openai"), "llm.example.test")

    def test_base_url_uses_provider_specific_fallback(self) -> None:
        env = {
            "OPENCODE_MODEL": "openai/gpt-4.1-mini",
            "OPENAI_BASE_URL": "https://openai-compatible.example/v1",
        }
        with patch.dict(os.environ, env, clear=True):
            self.assertEqual(
                env_utils.opencode_base_url("openai"),
                "https://openai-compatible.example/v1",
            )

    def test_config_json_contains_base_url_only_when_set(self) -> None:
        env = {
            "OPENCODE_MODEL": "openai/gpt-4.1-mini",
            "OPENCODE_BASE_URL": "https://llm.example.test/v1",
        }
        with patch.dict(os.environ, env, clear=True):
            config = env_utils.opencode_config_json("openai")
        self.assertIn('"baseURL": "https://llm.example.test/v1"', config)

    def test_config_json_omits_provider_without_base_url(self) -> None:
        with patch.dict(os.environ, {"OPENCODE_MODEL": "openai/gpt-4.1-mini"}, clear=True):
            config = env_utils.opencode_config_json("openai")
        self.assertNotIn('"provider"', config)

    def test_host_from_url_handles_empty_and_scheme_less_values(self) -> None:
        self.assertEqual(env_utils._host_from_url(""), "")
        self.assertEqual(env_utils._host_from_url("api.deepseek.com"), "api.deepseek.com")
        self.assertEqual(
            env_utils._host_from_url("https://api.example.test/v1"),
            "api.example.test",
        )

    def test_load_local_dotenv_warns_when_no_file_exists(self) -> None:
        stderr = StringIO()
        missing_dir = Path("/tmp/cube-opencode-missing-env-test")
        with patch.object(env_utils, "__file__", str(missing_dir / "env_utils.py")):
            with patch("pathlib.Path.cwd", return_value=missing_dir):
                with patch("sys.stderr", stderr):
                    env_utils.load_local_dotenv()
        self.assertIn("Warning: no .env file found", stderr.getvalue())

    def test_provider_inject_uses_bearer_header_for_openai_style_providers(self) -> None:
        self.assertEqual(
            env_utils.provider_inject("deepseek", "sk-test"),
            [{"header": "Authorization", "secret": "sk-test", "format": "Bearer ${SECRET}"}],
        )

    def test_provider_inject_uses_anthropic_headers(self) -> None:
        self.assertEqual(
            env_utils.provider_inject("anthropic", "sk-ant"),
            [
                {"header": "x-api-key", "secret": "sk-ant", "format": "${SECRET}"},
                {
                    "header": "anthropic-version",
                    "secret": "2023-06-01",
                    "format": "${SECRET}",
                },
            ],
        )


if __name__ == "__main__":
    unittest.main()
