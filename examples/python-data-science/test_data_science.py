# Copyright (c) 2026 Tencent Inc.
# SPDX-License-Identifier: Apache-2.0

from __future__ import annotations

import json
import os
import shutil
import sys
import tarfile
from pathlib import Path

import httpx
from dotenv import load_dotenv


EXAMPLE_DIR = Path(__file__).resolve().parent
SIDECAR_DIR = EXAMPLE_DIR.parent / "e2b-dev-sidecar"
if str(SIDECAR_DIR) not in sys.path:
    sys.path.insert(0, str(SIDECAR_DIR))

from dev_sidecar import setup_dev_sidecar


REMOTE_WORKDIR = "/tmp/cubesandbox-incident-rca"
REMOTE_ARCHIVE = "/tmp/cubesandbox-incident-rca-results.tar.gz"
LOCAL_OUTPUT_DIR = EXAMPLE_DIR / "output"
LOCAL_ARCHIVE = LOCAL_OUTPUT_DIR / "cubesandbox-incident-rca-results.tar.gz"


ROUND1_SCRIPT = r'''
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
'''


ROUND2_SCRIPT = r'''
import json
import os
import tarfile

import emoji
import humanize
import matplotlib.pyplot as plt
import pandas as pd


workdir = "/tmp/cubesandbox-incident-rca"
state_dir = os.path.join(workdir, "state")
output_dir = os.path.join(workdir, "results")
os.makedirs(output_dir, exist_ok=True)

with open(os.path.join(workdir, "runbook.json"), "r", encoding="utf-8") as f:
    runbook = json.load(f)
with open(os.path.join(state_dir, "baseline.json"), "r", encoding="utf-8") as f:
    baseline = json.load(f)
with open(os.path.join(workdir, "deployments.json"), "r", encoding="utf-8") as f:
    deployments = json.load(f)

metrics = pd.read_csv(os.path.join(state_dir, "metrics_enriched.csv"), encoding="utf-8")
alerts = pd.read_csv(os.path.join(workdir, "alerts.csv"), encoding="utf-8")
anomalies = pd.read_csv(os.path.join(state_dir, "anomaly_windows.csv"), encoding="utf-8-sig")

metrics["timestamp"] = pd.to_datetime(metrics["timestamp"])
alerts["timestamp"] = pd.to_datetime(alerts["timestamp"])
anomalies["timestamp"] = pd.to_datetime(anomalies["timestamp"])
for deployment in deployments:
    deployment["deployed_at"] = pd.to_datetime(deployment["deployed_at"])

first_breach = pd.to_datetime(baseline["first_breach"])
last_breach = pd.to_datetime(baseline["last_breach"])
window_start = first_breach - pd.Timedelta(minutes=runbook["correlation_window_minutes"])
correlated = [
    deployment
    for deployment in deployments
    if deployment["service"] == runbook["service"] and window_start <= deployment["deployed_at"] <= first_breach
]
primary_deployment = correlated[-1] if correlated else None

incident_minutes = int((last_breach - first_breach).total_seconds() / 60) + 5
latency_delta = baseline["incident_peak_p95_latency_ms"] / baseline["baseline_p95_latency_ms"]
error_delta = baseline["incident_peak_error_rate"] / baseline["baseline_error_rate"]

slo_summary = pd.DataFrame(
    [
        {
            "metric": "p95_latency_ms",
            "baseline": baseline["baseline_p95_latency_ms"],
            "incident_peak": baseline["incident_peak_p95_latency_ms"],
            "threshold": runbook["slo"]["latency_p95_ms"],
            "increase_ratio": round(latency_delta, 2),
        },
        {
            "metric": "error_rate",
            "baseline": baseline["baseline_error_rate"],
            "incident_peak": baseline["incident_peak_error_rate"],
            "threshold": runbook["slo"]["error_rate"],
            "increase_ratio": round(error_delta, 2),
        },
    ]
)
slo_summary_path = os.path.join(output_dir, "slo_summary.csv")
slo_summary.to_csv(slo_summary_path, index=False, encoding="utf-8-sig")

deployment_correlation = {
    "service": runbook["service"],
    "first_breach": first_breach.isoformat(),
    "correlation_window_minutes": runbook["correlation_window_minutes"],
    "matched_deployments": [
        {
            "service": item["service"],
            "version": item["version"],
            "deployed_at": item["deployed_at"].isoformat(),
            "minutes_before_breach": int((first_breach - item["deployed_at"]).total_seconds() / 60),
            "change": item["change"],
        }
        for item in correlated
    ],
    "likely_trigger": primary_deployment["version"] if primary_deployment else None,
    "confidence": "high" if primary_deployment and latency_delta > 2 and error_delta > 5 else "medium",
}
correlation_path = os.path.join(output_dir, "deployment_correlation.json")
with open(correlation_path, "w", encoding="utf-8") as f:
    json.dump(deployment_correlation, f, ensure_ascii=False, indent=2)

plt.rcParams["font.sans-serif"] = ["WenQuanYi Zen Hei"]
plt.rcParams["axes.unicode_minus"] = False

fig, axes = plt.subplots(2, 1, figsize=(12, 7), sharex=True)
axes[0].plot(metrics["timestamp"], metrics["p95_latency_ms"], color="#2563eb", linewidth=2, label="p95 延迟")
axes[0].axhline(runbook["slo"]["latency_p95_ms"], color="#dc2626", linestyle="--", label="延迟 SLO 阈值")
axes[0].fill_between(
    metrics["timestamp"],
    metrics["p95_latency_ms"],
    runbook["slo"]["latency_p95_ms"],
    where=metrics["p95_latency_ms"] > runbook["slo"]["latency_p95_ms"],
    color="#fecaca",
    alpha=0.45,
)
axes[0].set_ylabel("毫秒")
axes[0].set_title("checkout-api 延迟异常窗口")
axes[0].legend(loc="upper left")
axes[0].grid(axis="y", linestyle="--", alpha=0.35)

axes[1].plot(metrics["timestamp"], metrics["error_rate"] * 100, color="#16a34a", linewidth=2, label="错误率")
axes[1].axhline(runbook["slo"]["error_rate"] * 100, color="#dc2626", linestyle="--", label="错误率 SLO 阈值")
axes[1].fill_between(
    metrics["timestamp"],
    metrics["error_rate"] * 100,
    runbook["slo"]["error_rate"] * 100,
    where=metrics["error_rate"] > runbook["slo"]["error_rate"],
    color="#bbf7d0",
    alpha=0.45,
)
axes[1].set_ylabel("百分比")
axes[1].set_xlabel("时间")
axes[1].set_title("checkout-api 错误率异常窗口")
axes[1].legend(loc="upper left")
axes[1].grid(axis="y", linestyle="--", alpha=0.35)

for deployment in deployments:
    if deployment["service"] == runbook["service"]:
        for ax in axes:
            ax.axvline(deployment["deployed_at"], color="#7c3aed", linestyle=":", linewidth=1.5)
        axes[0].annotate(
            deployment["version"],
            xy=(deployment["deployed_at"], runbook["slo"]["latency_p95_ms"]),
            xytext=(6, 18),
            textcoords="offset points",
            fontsize=9,
            color="#5b21b6",
            rotation=30,
        )

fig.suptitle("AI Code Interpreter 生产事故 RCA 分析", fontsize=15)
fig.autofmt_xdate(rotation=25)
fig.tight_layout()
chart_path = os.path.join(output_dir, "事故分析图.png")
fig.savefig(chart_path, dpi=160)

report_path = os.path.join(output_dir, "incident_report.md")
duration_text = humanize.naturaldelta(pd.Timedelta(minutes=incident_minutes).to_pytimedelta())
with open(report_path, "w", encoding="utf-8") as f:
    f.write("# checkout-api 生产事故 RCA 报告\n\n")
    f.write(f"- 服务：{runbook['service']}\n")
    f.write(f"- 首次 SLO 违约：{first_breach.isoformat()}\n")
    f.write(f"- 最后异常点：{last_breach.isoformat()}\n")
    f.write(f"- 异常持续时间：{duration_text}\n")
    f.write(f"- 动态依赖验证：{emoji.emojize(':magnifying_glass_tilted_left: :package:')} `emoji` 与 `humanize` 在运行中安装后参与报告生成\n\n")
    f.write("## 结论\n\n")
    if primary_deployment:
        f.write(
            f"最可疑触发因素是 `{primary_deployment['version']}`，该版本在首次 SLO 违约前 "
            f"{int((first_breach - primary_deployment['deployed_at']).total_seconds() / 60)} 分钟部署，变更内容为："
            f"{primary_deployment['change']}。\n\n"
        )
    else:
        f.write("未在关联窗口内发现同服务部署事件，建议继续检查上游依赖和流量来源。\n\n")
    f.write("## 关键指标\n\n")
    f.write(slo_summary.to_markdown(index=False))
    f.write("\n\n")
    f.write("## 告警时间线\n\n")
    f.write(alerts[["timestamp", "severity", "alert", "message"]].to_markdown(index=False))
    f.write("\n\n")
    f.write("## 建议动作\n\n")
    f.write(f"1. 按 runbook 建议执行：{runbook['incident_policy']['rollback_hint']}。\n")
    f.write("2. 对 coupon validation 与 inventory fan-out 路径增加分位延迟和错误码维度监控。\n")
    f.write("3. 将本次 `deployment_correlation.json` 和 `slo_summary.csv` 附到事故复盘工单。\n")

manifest_path = os.path.join(output_dir, "manifest.json")
manifest = {
    "scenario": "ai_incident_rca_code_interpreter",
    "service": runbook["service"],
    "input_files": [
        "incident_metrics.csv",
        "deployments.json",
        "alerts.csv",
        "runbook.json",
    ],
    "stateful_rounds": [
        "round1_anomaly_detection",
        "round2_followup_rca_packaging",
    ],
    "first_breach": first_breach.isoformat(),
    "last_breach": last_breach.isoformat(),
    "incident_minutes": incident_minutes,
    "likely_trigger": deployment_correlation["likely_trigger"],
    "artifacts": [
        "事故分析图.png",
        "incident_report.md",
        "slo_summary.csv",
        "deployment_correlation.json",
        "manifest.json",
    ],
}
with open(manifest_path, "w", encoding="utf-8") as f:
    json.dump(manifest, f, ensure_ascii=False, indent=2)

archive_path = "/tmp/cubesandbox-incident-rca-results.tar.gz"
with tarfile.open(archive_path, "w:gz") as tar:
    for name in manifest["artifacts"]:
        tar.add(os.path.join(output_dir, name), arcname=name)
    tar.add(os.path.join(state_dir, "anomaly_windows.csv"), arcname="state/anomaly_windows.csv")
    tar.add(os.path.join(state_dir, "baseline.json"), arcname="state/baseline.json")

print("Round 2 completed: RCA report and packaged artifacts generated")
print("Likely trigger:", deployment_correlation["likely_trigger"])
print("Incident minutes:", incident_minutes)
print("Generated files:")
for name in manifest["artifacts"]:
    print("-", os.path.join(output_dir, name))
print("-", archive_path)
'''


def _load_environment() -> None:
    env_path = EXAMPLE_DIR / ".env"
    load_dotenv(env_path if env_path.exists() else EXAMPLE_DIR / "env.example")


def _read_bytes(path: Path) -> bytes:
    return path.read_bytes()


def _write_sandbox_file(sandbox, remote_path: str, local_path: Path) -> None:
    sandbox.files.write(remote_path, _read_bytes(local_path).decode("utf-8"))


def _safe_extract_tar(tar: tarfile.TarFile, target_dir: Path) -> None:
    target_root = target_dir.resolve()
    for member in tar.getmembers():
        member_path = (target_dir / member.name).resolve()
        if target_root != member_path and target_root not in member_path.parents:
            raise RuntimeError(f"Archive member escapes output directory: {member.name}")

    if sys.version_info >= (3, 12):
        tar.extractall(target_dir, filter="data")
    else:
        tar.extractall(target_dir)


def _download_archive(sandbox) -> None:
    LOCAL_OUTPUT_DIR.mkdir(exist_ok=True)
    with httpx.stream("GET", sandbox.download_url(REMOTE_ARCHIVE), timeout=60) as response:
        response.raise_for_status()
        with open(LOCAL_ARCHIVE, "wb") as f:
            for chunk in response.iter_raw():
                f.write(chunk)

    extract_dir = LOCAL_OUTPUT_DIR / "results"
    if extract_dir.exists():
        shutil.rmtree(extract_dir)
    extract_dir.mkdir()
    with tarfile.open(LOCAL_ARCHIVE, "r:gz") as tar:
        _safe_extract_tar(tar, extract_dir)

    expected = [
        "事故分析图.png",
        "incident_report.md",
        "slo_summary.csv",
        "deployment_correlation.json",
        "manifest.json",
    ]
    missing = [name for name in expected if not (extract_dir / name).exists()]
    if missing:
        raise RuntimeError(f"Downloaded archive is missing files: {missing}")

    manifest = json.loads((extract_dir / "manifest.json").read_text(encoding="utf-8"))
    if (
        manifest.get("scenario") != "ai_incident_rca_code_interpreter"
        or not manifest.get("likely_trigger")
        or "round2_followup_rca_packaging" not in manifest.get("stateful_rounds", [])
    ):
        raise RuntimeError("Downloaded manifest does not match the expected incident RCA output")


def main() -> None:
    _load_environment()
    setup_dev_sidecar()

    from e2b import Sandbox

    template_id = os.environ.get("CUBE_TEMPLATE_ID")
    api_url = os.environ.get("E2B_API_URL", "http://127.0.0.1:13000")

    if not template_id:
        print("[ERROR] Please set CUBE_TEMPLATE_ID in .env or your shell environment.")
        sys.exit(1)

    print(f"Connecting to Cube Sandbox API at: {api_url}")
    print(f"Booting incident RCA code interpreter sandbox from template: {template_id}")

    with Sandbox(template=template_id) as sandbox:
        print(f"[Success] Sandbox {sandbox.sandbox_id} is running")

        print("\n--- Step 1: Upload incident inputs and analysis programs ---")
        sandbox.commands.run(f"mkdir -p {REMOTE_WORKDIR}")
        for filename in ["incident_metrics.csv", "deployments.json", "alerts.csv", "runbook.json"]:
            _write_sandbox_file(sandbox, f"{REMOTE_WORKDIR}/{filename}", EXAMPLE_DIR / filename)
        sandbox.files.write(f"{REMOTE_WORKDIR}/round1_detect.py", ROUND1_SCRIPT)
        sandbox.files.write(f"{REMOTE_WORKDIR}/round2_rca.py", ROUND2_SCRIPT)
        print("Uploaded metrics, deployments, alerts, runbook, and two analysis rounds")

        print("\n--- Step 2: Dynamically install follow-up analysis dependencies ---")
        install = sandbox.commands.run("pip3 install --no-cache-dir emoji humanize")
        if install.exit_code != 0:
            print(install.stderr)
            raise RuntimeError("Failed to install dynamic dependencies: emoji humanize")
        print("[Success] Installed emoji and humanize inside the running sandbox")

        print("\n--- Step 3: Round 1 anomaly detection writes sandbox state ---")
        round1 = sandbox.commands.run(f"python3 {REMOTE_WORKDIR}/round1_detect.py")
        if round1.exit_code != 0:
            print(round1.stdout)
            print(round1.stderr)
            raise RuntimeError(f"Round 1 failed with exit code {round1.exit_code}")
        print(round1.stdout)

        print("\n--- Step 4: Round 2 follow-up RCA reuses state and packages artifacts ---")
        round2 = sandbox.commands.run(f"python3 {REMOTE_WORKDIR}/round2_rca.py")
        if round2.exit_code != 0:
            print(round2.stdout)
            print(round2.stderr)
            raise RuntimeError(f"Round 2 failed with exit code {round2.exit_code}")
        print(round2.stdout)

        print("\n--- Step 5: Download and extract packaged RCA results ---")
        _download_archive(sandbox)
        print(f"[Success] Archive downloaded to: {LOCAL_ARCHIVE}")
        print(f"[Success] Extracted files to: {LOCAL_OUTPUT_DIR / 'results'}")

    print("\n[Finished] Incident RCA code interpreter flow completed successfully")


if __name__ == "__main__":
    main()
