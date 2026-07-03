---
title: Python Incident RCA Code Interpreter Sandbox
author: Sam
date: 2026-07-04
tags:
  - python
  - data-science
  - code-interpreter
  - incident-rca
  - e2b
lang: en-US
---

# Python Incident RCA Code Interpreter Sandbox

## Business Context

AI agents are increasingly used to help SRE and platform teams triage incidents. During an outage, engineers often export service metrics, deployment events, alerts, and runbook thresholds, then ask the agent to identify the incident window, correlate changes, and draft a report.

The `examples/python-data-science` template demonstrates this workflow inside Cube Sandbox. The agent-style client uploads multiple files, runs Python in an isolated sandbox, keeps intermediate analysis state in the sandbox workspace, and downloads a packaged RCA result.

## Key Challenges

- Incident analysis needs real data processing, not just natural-language guessing.
- Metrics, alerts, deployment events, and runbook policies usually arrive as separate files.
- Code interpreter sessions need a persistent workspace across multiple analysis turns.
- Chinese reports and charts require CJK fonts in the sandbox image.
- Follow-up analysis may need packages that were not known when the image was built.
- RCA outputs should be structured enough to attach to tickets, review docs, or downstream automation.

## Solution with Cube Sandbox

The template builds from `ghcr.io/tencentcloud/cubesandbox-base:latest` and installs a Python data-science stack plus `fonts-wqy-zenhei`. The demo starts a sandbox through the E2B-compatible SDK and uploads:

- `incident_metrics.csv`: time-series service metrics,
- `deployments.json`: deployment events,
- `alerts.csv`: alert timeline,
- `runbook.json`: SLO thresholds and response hints.

The workflow then runs two analysis rounds in the same sandbox:

1. **Anomaly detection round**: calculates error rate, detects SLO breaches, and writes intermediate state under `/tmp/cubesandbox-incident-rca/state`.
2. **Follow-up RCA round**: reuses that state, correlates the incident with deployment events, renders a Chinese chart, writes a Markdown RCA report, emits structured JSON/CSV artifacts, and packages everything into a tarball.

The client downloads the archive, extracts it locally, and validates the manifest. This mirrors the way an AI code interpreter handles multi-file input, stateful analysis, and downloadable artifacts while keeping code execution isolated.

## Results and Benefits

- Provides a reusable starting point for AI incident-analysis sandboxes.
- Demonstrates a stateful multi-turn code-interpreter workflow without requiring host-side file mutation.
- Shows dynamic dependency expansion through `pip3 install emoji humanize` inside the sandbox.
- Produces reviewable artifacts: chart, RCA report, SLO summary, deployment correlation JSON, manifest, and intermediate state.
- Demonstrates Chinese Matplotlib rendering for localized engineering reports.
- Registers the example in the tutorial index so new users can discover it alongside other templates.

## References

- Related example: `examples/python-data-science`
- Related issue: [CubeSandbox sandbox templates and example ecosystem](https://github.com/TencentCloud/CubeSandbox/issues/645)
