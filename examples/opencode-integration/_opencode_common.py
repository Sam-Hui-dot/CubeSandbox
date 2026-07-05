# Copyright (c) 2026 Tencent Inc.
# SPDX-License-Identifier: Apache-2.0

"""Shared sandbox command helpers for the OpenCode integration examples."""

from __future__ import annotations

import inspect
import os
import sys
from collections.abc import Callable
from functools import lru_cache
from typing import Any


SECRET_ENV_MARKERS = ("API_KEY", "TOKEN", "SECRET")


def stream_writer(stream) -> Callable[[object], None]:
    def write(chunk: object) -> None:
        text = getattr(chunk, "line", chunk)
        stream.write(str(text))
        stream.flush()

    return write


def run_command(
    sandbox: Any,
    command: str,
    *,
    cwd: str | None = None,
    envs: dict[str, str] | None = None,
    timeout: int | float | None = None,
    stream: bool = False,
    user: str = "root",
):
    kwargs = {"cwd": cwd, "timeout": timeout, "user": user}
    kwargs = {key: value for key, value in kwargs.items() if value is not None}
    if envs:
        kwargs[_command_env_kwarg(type(sandbox.commands))] = envs
    if stream:
        kwargs["on_stdout"] = stream_writer(sys.stdout)
        kwargs["on_stderr"] = stream_writer(sys.stderr)
    kwargs = _filter_supported_kwargs(type(sandbox.commands), kwargs)

    return sandbox.commands.run(command, **kwargs)


def ensure_success(result, action: str) -> None:
    exit_code = getattr(result, "exit_code", None)
    if exit_code not in (None, 0):
        stdout = redact_secrets(getattr(result, "stdout", ""))
        stderr = redact_secrets(getattr(result, "stderr", ""))
        raise SystemExit(
            f"Failed to {action} (exit {exit_code}).\nSTDOUT:\n{stdout}\nSTDERR:\n{stderr}"
        )


def sandbox_identifier(sandbox: Any) -> str:
    return getattr(sandbox, "sandbox_id", getattr(sandbox, "id", "unknown"))


@lru_cache(maxsize=None)
def _command_env_kwarg(commands_type: type) -> str:
    params = _command_run_parameters(commands_type)
    if params is None:
        return "envs"
    if "envs" in params:
        return "envs"
    if "env" in params:
        return "env"
    return "envs"


@lru_cache(maxsize=None)
def _command_run_parameters(commands_type: type) -> dict[str, inspect.Parameter] | None:
    try:
        return dict(inspect.signature(commands_type.run).parameters)
    except (TypeError, ValueError):
        return None


def _filter_supported_kwargs(commands_type: type, kwargs: dict[str, Any]) -> dict[str, Any]:
    params = _command_run_parameters(commands_type)
    if params is None:
        return kwargs
    if any(param.kind == inspect.Parameter.VAR_KEYWORD for param in params.values()):
        return kwargs
    return {key: value for key, value in kwargs.items() if key in params}


def _is_secret_env(name: str) -> bool:
    return any(marker in name.upper() for marker in SECRET_ENV_MARKERS)


def _secret_is_present(value: str | None) -> bool:
    return bool(value and value.strip() and not value.strip().startswith("<"))


def redact_secrets(text: str) -> str:
    redacted = text
    for name, value in os.environ.items():
        if _is_secret_env(name) and _secret_is_present(value):
            redacted = redacted.replace(value, "<redacted>")
    return redacted
