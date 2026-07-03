# Python Incident RCA Code Interpreter Sandbox

This example shows how to build and verify a CubeSandbox template for AI Agent code-interpreter workflows. It uses a production incident analysis scenario instead of a toy calculation: the host uploads service metrics, deployment events, alerts, and a runbook; the sandbox runs two Python analysis rounds in the same workspace; then it produces a Chinese RCA report, chart, structured summaries, and a packaged result archive.

## What It Demonstrates

1. **Industrial data-science workflow**: Pandas analyzes service metrics, SLO thresholds, deployment events, and alert timelines to identify an incident window and likely trigger.
2. **Stateful code-interpreter workspace**: round 1 writes intermediate state under `/tmp/cubesandbox-incident-rca/state`; round 2 reuses that state in the same sandbox to generate the final RCA package.
3. **Chinese Matplotlib rendering**: the image installs `fonts-wqy-zenhei` and the analysis script configures Matplotlib to use `WenQuanYi Zen Hei`, so Chinese chart titles and labels render correctly.
4. **Dynamic package installation**: the client installs `emoji` and `humanize` with `pip3 install` inside the running sandbox, then imports them in the follow-up analysis round.
5. **Multi-file input and packaged output**: the sandbox generates a Chinese incident chart, Markdown RCA report, SLO summary CSV, deployment-correlation JSON, manifest JSON, and a downloadable tarball.

## Files

- `Dockerfile`: builds the reusable Python data-science sandbox image.
- `incident_metrics.csv`: time-series service metrics for `checkout-api`.
- `deployments.json`: deployment events used for correlation analysis.
- `alerts.csv`: alert timeline exported from the monitoring system.
- `runbook.json`: SLO thresholds and incident policy hints.
- `test_data_science.py`: end-to-end client that creates a sandbox, uploads files, runs two analysis rounds, downloads the result archive, and validates the manifest.
- `env.example`: local CubeSandbox API/proxy settings and template ID placeholder.

## Step 1: Build the Image

Build the image where the Cube node runtime can access it:

```bash
docker build -t cubesandbox-data-science:latest .
```

The image includes Python, Pandas, Matplotlib, scientific dependencies, and Chinese fonts. It is larger than a minimal image because it is intended for code-interpreter style workloads.

## Step 2: Register a CubeSandbox Template

Inside the CubeSandbox dev VM or on the node where `cubemastercli` is installed, register the image:

```bash
cubemastercli tpl create-from-image \
    --image               cubesandbox-data-science:latest \
    --writable-layer-size 2G \
    --expose-port         49983 \
    --probe               49983 \
    --probe-path          /health
```

Copy the returned template ID, for example `tpl-xxxxxxxxxxxxxxxxxxxxxxxx`.

## Step 3: Configure the Client

Install client requirements:

```bash
pip3 install -r requirements.txt
```

Create `.env` and set the template ID:

```bash
cp env.example .env
```

For the local dev VM, `.env` should look like this:

```bash
E2B_API_URL="http://127.0.0.1:13000"
CUBE_REMOTE_PROXY_BASE="https://127.0.0.1:11443"
CUBE_TEMPLATE_ID="tpl-xxxxxxxxxxxxxxxxxxxxxxxx"
E2B_API_KEY=e2b_dummyapikeyforlocaltest
```

## Step 4: Run the End-to-End Demo

```bash
python3 test_data_science.py
```

The script will:

1. Start a sandbox from the registered template.
2. Upload `incident_metrics.csv`, `deployments.json`, `alerts.csv`, `runbook.json`, and two generated analysis scripts.
3. Install `emoji` and `humanize` dynamically inside the sandbox.
4. Run round 1 anomaly detection and save intermediate state in the sandbox workspace.
5. Run round 2 RCA analysis using the saved state, deployment events, alerts, and runbook policy.
6. Create `/tmp/cubesandbox-incident-rca-results.tar.gz` in the sandbox.
7. Download and extract the archive to `output/results/`.

Expected extracted files:

- `事故分析图.png`: Chinese incident chart proving font rendering works.
- `incident_report.md`: Markdown RCA report with likely trigger and suggested actions.
- `slo_summary.csv`: baseline vs incident SLO summary.
- `deployment_correlation.json`: structured deployment-to-incident correlation.
- `manifest.json`: machine-readable artifact manifest and key metrics.
- `state/anomaly_windows.csv` and `state/baseline.json`: intermediate state produced by round 1 and packaged for auditability.
