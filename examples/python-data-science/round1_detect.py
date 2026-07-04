import json
import os

import pandas as pd


workdir = "/tmp/cubesandbox-incident-rca"
metrics_path = os.path.join(workdir, "incident_metrics.csv")
runbook_path = os.path.join(workdir, "runbook.json")
state_dir = os.path.join(workdir, "state")
os.makedirs(state_dir, exist_ok=True)

with open(runbook_path, "r", encoding="utf-8") as f:
    runbook = json.load(f)

metrics = pd.read_csv(metrics_path, encoding="utf-8")
metrics["timestamp"] = pd.to_datetime(metrics["timestamp"])
metrics["error_rate"] = metrics["error_count"] / metrics["requests"]
metrics["latency_breach"] = metrics["p95_latency_ms"] > runbook["slo"]["latency_p95_ms"]
metrics["error_breach"] = metrics["error_rate"] > runbook["slo"]["error_rate"]
metrics["slo_breach"] = metrics["latency_breach"] | metrics["error_breach"]

breaches = metrics[metrics["slo_breach"]].copy()
if breaches.empty:
    raise RuntimeError("No SLO breach detected; the demo dataset should contain an incident window.")

first_breach = breaches["timestamp"].min()
last_breach = breaches["timestamp"].max()
baseline = metrics[metrics["timestamp"] < first_breach].tail(12)

baseline_summary = {
    "service": runbook["service"],
    "first_breach": first_breach.isoformat(),
    "last_breach": last_breach.isoformat(),
    "baseline_p95_latency_ms": round(float(baseline["p95_latency_ms"].median()), 2),
    "incident_peak_p95_latency_ms": int(metrics["p95_latency_ms"].max()),
    "baseline_error_rate": round(float(baseline["error_rate"].mean()), 5),
    "incident_peak_error_rate": round(float(metrics["error_rate"].max()), 5),
    "breach_points": int(len(breaches)),
}

anomaly_windows = metrics.loc[
    metrics["slo_breach"],
    [
        "timestamp",
        "service",
        "requests",
        "error_count",
        "error_rate",
        "p95_latency_ms",
        "cpu_pct",
        "latency_breach",
        "error_breach",
    ],
].copy()
anomaly_windows["timestamp"] = anomaly_windows["timestamp"].dt.strftime("%Y-%m-%dT%H:%M:%S")
anomaly_windows.to_csv(os.path.join(state_dir, "anomaly_windows.csv"), index=False, encoding="utf-8-sig")

with open(os.path.join(state_dir, "baseline.json"), "w", encoding="utf-8") as f:
    json.dump(baseline_summary, f, ensure_ascii=False, indent=2)

metrics.to_csv(os.path.join(state_dir, "metrics_enriched.csv"), index=False, encoding="utf-8-sig")

print("Round 1 completed: anomaly detection state saved")
print(json.dumps(baseline_summary, ensure_ascii=False, indent=2))
