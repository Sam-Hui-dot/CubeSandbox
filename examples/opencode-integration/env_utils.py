# Copyright (c) 2026 Tencent Inc.
# SPDX-License-Identifier: Apache-2.0

from __future__ import annotations

import json
import os
import shlex
import sys
from pathlib import Path
from urllib.parse import urlparse

from dotenv import load_dotenv

DEFAULT_WORKSPACE = "/workspace"

PROVIDER_KEY_ENV = {
    "anthropic": "ANTHROPIC_API_KEY",
    "deepseek": "DEEPSEEK_API_KEY",
    "openai": "OPENAI_API_KEY",
    "openrouter": "OPENROUTER_API_KEY",
}

PROVIDER_DEFAULT_HOST = {
    "anthropic": "api.anthropic.com",
    "deepseek": "api.deepseek.com",
    "openai": "api.openai.com",
    "openrouter": "openrouter.ai",
}

PASSTHROUGH_ENV_NAMES = (
    "ANTHROPIC_API_KEY",
    "DEEPSEEK_API_KEY",
    "OPENAI_API_KEY",
    "OPENROUTER_API_KEY",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
)


def load_local_dotenv() -> None:
    candidate_paths = [Path(__file__).with_name(".env"), Path.cwd() / ".env"]
    seen_paths: set[Path] = set()
    for path in candidate_paths:
        resolved = path.resolve()
        if resolved in seen_paths:
            continue
        seen_paths.add(resolved)
        if path.is_file():
            load_dotenv(dotenv_path=path, override=False)
            return


def required(name: str) -> str:
    value = os.environ.get(name)
    if not value:
        raise SystemExit(f"Missing required environment variable: {name}")
    return value


def optional(name: str, default: str = "") -> str:
    return os.environ.get(name) or default


def int_env(name: str, default: int) -> int:
    raw = os.environ.get(name)
    if not raw:
        return default
    try:
        return int(raw)
    except ValueError as exc:
        raise SystemExit(f"{name} must be an integer, got {raw!r}") from exc


def opencode_workspace() -> str:
    return optional("OPENCODE_WORKSPACE", DEFAULT_WORKSPACE)


def opencode_model() -> str:
    model = required("OPENCODE_MODEL").strip()
    if "/" not in model:
        raise SystemExit(
            "OPENCODE_MODEL must be in provider/model form, for example openai/gpt-4.1-mini"
        )
    return model


def opencode_provider(model: str | None = None) -> str:
    selected = model or opencode_model()
    return selected.split("/", 1)[0].strip().lower()


def provider_key_name(provider: str | None = None) -> str:
    provider_name = provider or opencode_provider()
    return PROVIDER_KEY_ENV.get(provider_name, f"{provider_name.upper()}_API_KEY")


def require_provider_key(provider: str | None = None) -> str:
    key_name = provider_key_name(provider)
    value = os.environ.get(key_name)
    if not value:
        raise SystemExit(f"Missing required environment variable: {key_name}")
    return value


def opencode_base_url(provider: str | None = None) -> str:
    provider_name = provider or opencode_provider()
    explicit = os.environ.get("OPENCODE_BASE_URL")
    if explicit:
        return explicit
    provider_specific = os.environ.get(f"{provider_name.upper()}_BASE_URL")
    return provider_specific or ""


def opencode_llm_host(provider: str | None = None) -> str:
    explicit = os.environ.get("OPENCODE_LLM_HOST")
    if explicit:
        return _host_from_url(explicit)
    base_url = opencode_base_url(provider)
    if base_url:
        return _host_from_url(base_url)
    return PROVIDER_DEFAULT_HOST.get(provider or opencode_provider(), "")


def build_opencode_env(include_secrets: bool = True) -> dict[str, str]:
    provider = opencode_provider()
    env = {
        "OPENCODE_DISABLE_AUTO_UPDATE": optional("OPENCODE_DISABLE_AUTO_UPDATE", "1"),
    }
    for name in PASSTHROUGH_ENV_NAMES:
        value = os.environ.get(name)
        if not value:
            continue
        if name.endswith("_API_KEY") and not include_secrets:
            continue
        env[name] = value
    return env


def opencode_config_json(provider: str | None = None) -> str:
    provider_name = provider or opencode_provider()
    base_url = opencode_base_url(provider_name)
    config: dict[str, object] = {"$schema": "https://opencode.ai/config.json"}
    if base_url:
        config["provider"] = {
            provider_name: {
                "options": {
                    "baseURL": base_url,
                }
            }
        }
    return json.dumps(config, indent=2, sort_keys=True)


def opencode_command(
    prompt: str,
    *,
    workspace: str | None = None,
    model: str | None = None,
    title: str | None = None,
    session: str | None = None,
    continue_last: bool = False,
    auto: bool = True,
    json_format: bool = True,
) -> str:
    args = ["opencode", "run"]
    if model:
        args.extend(["--model", model])
    if workspace:
        args.extend(["--dir", workspace])
    if title:
        args.extend(["--title", title])
    if session:
        args.extend(["--session", session])
    elif continue_last:
        args.append("--continue")
    if auto:
        args.append("--auto")
    if json_format:
        args.extend(["--format", "json"])
    args.append(prompt)
    return " ".join(shlex.quote(arg) for arg in args)


def shell_join(*parts: str) -> str:
    return " && ".join(part for part in parts if part)


def setup_dev_sidecar_if_requested() -> None:
    value = os.environ.get("CUBE_DEV_SIDECAR", "").strip().lower()
    if value not in ("1", "true", "yes", "on"):
        return
    sidecar_dir = Path(__file__).resolve().parents[1] / "e2b-dev-sidecar"
    if not sidecar_dir.is_dir():
        raise SystemExit(
            "CUBE_DEV_SIDECAR=1 was set, but examples/e2b-dev-sidecar was not found."
        )
    if str(sidecar_dir) not in sys.path:
        sys.path.insert(0, str(sidecar_dir))
    from dev_sidecar import setup_dev_sidecar

    setup_dev_sidecar()


def _host_from_url(value: str) -> str:
    candidate = value.strip()
    if not candidate:
        return ""
    if "://" not in candidate:
        candidate = f"https://{candidate}"
    return urlparse(candidate).hostname or ""


def provider_inject(provider: str, secret: str) -> list[dict[str, str]]:
    provider_name = provider.strip().lower()
    if provider_name == "anthropic":
        return [
            {"header": "x-api-key", "secret": secret, "format": "${SECRET}"},
            {
                "header": "anthropic-version",
                "secret": "2023-06-01",
                "format": "${SECRET}",
            },
        ]
    return [{"header": "Authorization", "secret": secret, "format": "Bearer ${SECRET}"}]
