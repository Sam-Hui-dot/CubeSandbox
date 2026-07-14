# Copyright (c) 2026 Tencent Inc.
# SPDX-License-Identifier: Apache-2.0

from __future__ import annotations

import os
import sys
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))

import network_policy


class NetworkPolicyTest(unittest.TestCase):
    def test_requires_native_cubesandbox_sdk_env(self) -> None:
        env = {
            "E2B_API_URL": "http://127.0.0.1:3000",
            "E2B_API_KEY": "e2b_000000",
        }
        with patch.dict(os.environ, env, clear=True):
            with self.assertRaisesRegex(SystemExit, "CUBE_API_URL"):
                network_policy.require_native_sdk_env()

    def test_accepts_native_cubesandbox_sdk_env(self) -> None:
        env = {
            "CUBE_API_URL": "http://127.0.0.1:3000",
            "CUBE_PROXY_NODE_IP": "127.0.0.1",
        }
        with patch.dict(os.environ, env, clear=True):
            network_policy.require_native_sdk_env()


if __name__ == "__main__":
    unittest.main()
