# OpenCode + CubeSandbox Example

[中文](README_zh.md)

Run [OpenCode](https://opencode.ai/) inside a CubeSandbox MicroVM. The host
driver creates an isolated sandbox, asks OpenCode to edit a tiny Python project,
checks the generated artifacts, and demonstrates pause/resume plus restricted
LLM egress.

## What is included

```text
opencode-integration/
|-- Dockerfile              # CubeSandbox template image with Node.js + OpenCode
|-- .env.example            # Copy to .env and fill in local values
|-- build-template.sh       # Prints docker/cubemastercli commands
|-- env_utils.py            # Provider, model, env, and command helpers
|-- _opencode_common.py     # Sandbox command helpers
|-- run_opencode.py         # One-shot coding-agent demo
|-- resume_opencode.py      # pause/resume session persistence demo
|-- network_policy.py       # Default-deny egress + CubeEgress injection demo
|-- test_env_utils.py       # Unit tests for config handling
|-- test_commands.py        # Unit tests for command construction
`-- requirements.txt        # Host-side Python dependencies
```

## Prerequisites

- A running CubeSandbox deployment with CubeAPI reachable at `http://<node>:3000`.
- `cubemastercli` connected to the cluster.
- Docker and a registry reachable by Cube nodes.
- Python 3.10+ on the host.
- An LLM provider key supported by OpenCode.

## 1. Build the image

```bash
cd examples/opencode-integration
docker build --platform linux/amd64 -t <registry>/opencode-cube:latest .
docker push <registry>/opencode-cube:latest
```

The image installs Node.js 22 and `opencode-ai` on top of
`ghcr.io/tencentcloud/cubesandbox-base:2026.16`.

## 2. Register a CubeSandbox template

```bash
cubemastercli tpl create-from-image \
  --image <registry>/opencode-cube:latest \
  --writable-layer-size 4G \
  --expose-port 49983 \
  --probe 49983 \
  --probe-path /health
```

Wait for the template job to become `READY`, then copy the returned
`template_id` into `.env`.

## 3. Configure the host driver

```bash
cp .env.example .env
pip install -r requirements.txt
```

Required values:

| Variable | Description |
|---|---|
| `E2B_API_URL` | CubeAPI URL, for example `http://<node>:3000` |
| `E2B_API_KEY` | Any non-empty value for local dev, or the real key when auth is enabled |
| `CUBE_TEMPLATE_ID` | Template ID created in step 2 |
| `OPENCODE_MODEL` | OpenCode model in `provider/model` form |
| `<PROVIDER>_API_KEY` | API key matching the provider prefix |
| `CUBE_API_URL` | Required for `network_policy.py`; native CubeSandbox SDK CubeAPI URL |
| `CUBE_PROXY_NODE_IP` | Required for `network_policy.py`; CubeProxy IP used for command streams |

Use `OPENCODE_BASE_URL` for OpenAI-compatible custom endpoints. If it is not
set, the scripts also accept provider-specific variables such as
`OPENAI_BASE_URL`. The scripts write an `opencode.json` into the sandbox
workspace with `provider.<provider_prefix>.options.baseURL` when either value
is set.

When running against this repository's `dev-env/` VM, also set:

```bash
CUBE_DEV_SIDECAR=1
```

The sidecar patches the E2B SDK so sandbox traffic is routed through the
dev-env CubeProxy port forwards.

`network_policy.py` uses the native `cubesandbox` SDK instead of the E2B SDK, so
it reads `CUBE_API_URL` and `CUBE_PROXY_NODE_IP`. When running that script
against `dev-env/`, use the forwarded native SDK API and proxy variables:

```bash
CUBE_API_URL=http://127.0.0.1:13000
CUBE_PROXY_NODE_IP=127.0.0.1
CUBE_PROXY_PORT_HTTP=11080
```

## 4. Run the one-shot coding task

```bash
python3 run_opencode.py --dry-run
python3 run_opencode.py
```

The demo seeds `/workspace` with a tiny Python project, asks OpenCode to
implement `calculator.add`, runs `python3 -m unittest discover -v`, and verifies
that `result.md` contains `OPENCODE_CUBE_OK`.

## 5. Verify pause/resume persistence

```bash
python3 resume_opencode.py
```

The first turn asks OpenCode to write `plan.md`, pauses the sandbox, reconnects
to the same sandbox ID, verifies `/workspace` and OpenCode state survived, then
continues the task and checks for `OPENCODE_RESUME_OK`.

## 6. Run with restricted egress

```bash
python3 network_policy.py
```

This path uses the native `cubesandbox` SDK to create the sandbox with
default-deny egress and an allow rule for only the configured LLM host. The
provider key is injected by CubeEgress as an HTTP header, while the sandbox sees
only a placeholder environment value.

Set `OPENCODE_LLM_HOST` when using a custom endpoint:

```bash
OPENCODE_LLM_HOST=api.openai.com python3 network_policy.py
```

For quick policy checks without spending LLM tokens:

```bash
python3 network_policy.py --skip-agent
```

## Validation

```bash
python3 -m py_compile env_utils.py _opencode_common.py run_opencode.py resume_opencode.py network_policy.py
python3 -m unittest discover . -p 'test_*.py'
bash -n build-template.sh
```

Optional image checks:

```bash
docker build --platform linux/amd64 -t opencode-cube:verify .
docker run --rm opencode-cube:verify opencode --version
docker run --rm opencode-cube:verify node --version
docker run --rm opencode-cube:verify python3 --version
```

## Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| `Missing required environment variable` | `.env` is incomplete | Fill every required field in `.env` |
| `OPENCODE_MODEL must be in provider/model form` | Model lacks provider prefix | Use values like `openai/gpt-4.1-mini` |
| OpenCode cannot authenticate | Wrong provider key name | Match the prefix: `openai/*` needs `OPENAI_API_KEY` |
| Template probe fails | The envd port is not reachable | Use `--probe 49983 --probe-path /health` with the CubeSandbox base entrypoint |
| `403 Forbidden - CubeEgress` | Strict egress blocked a host | Set `OPENCODE_LLM_HOST` to the real provider host |
| Supply-chain hardening required | The example Dockerfile uses the NodeSource setup script for readability | Pin and verify Node.js packages or mirror the artifacts in production pipelines |

## Security notes

- The Docker image never contains provider secrets.
- `run_opencode.py` and `resume_opencode.py` inject the provider key only for
  the command and redact known secret values from failure output. This is
  convenient for local validation but leaves egress open.
- `network_policy.py` is the recommended shared-cluster pattern: default-deny
  egress plus CubeEgress header injection.
- Do not commit `.env`.
