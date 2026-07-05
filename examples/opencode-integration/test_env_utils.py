# Copyright (c) 2026 Tencent Inc.
# SPDX-License-Identifier: Apache-2.0

from __future__ import annotations

import os
import unittest
from unittest.mock import patch

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

    def test_llm_host_prefers_base_url(self) -> None:
        env = {
            "OPENCODE_MODEL": "openai/gpt-4.1-mini",
            "OPENCODE_BASE_URL": "https://llm.example.test/v1",
        }
        with patch.dict(os.environ, env, clear=True):
            self.assertEqual(env_utils.opencode_llm_host("openai"), "llm.example.test")

    def test_config_json_contains_base_url_only_when_set(self) -> None:
        env = {
            "OPENCODE_MODEL": "openai/gpt-4.1-mini",
            "OPENCODE_BASE_URL": "https://llm.example.test/v1",
        }
        with patch.dict(os.environ, env, clear=True):
            config = env_utils.opencode_config_json("openai")
        self.assertIn('"baseURL": "https://llm.example.test/v1"', config)


if __name__ == "__main__":
    unittest.main()
