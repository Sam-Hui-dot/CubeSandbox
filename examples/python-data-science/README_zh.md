# Python 生产事故 RCA 代码解释器沙箱

本示例展示如何为 AI Agent Code Interpreter 场景构建并验证一个 CubeSandbox 模板。它不再使用玩具数据，而是模拟生产事故分析：宿主机上传服务指标、部署事件、告警和 runbook，沙箱在同一个工作区中执行两轮 Python 分析，最终生成中文 RCA 报告、图表、结构化摘要和可下载结果包。

## 功能亮点

1. **工业数据分析工作流**：使用 Pandas 分析服务指标、SLO 阈值、部署事件和告警时间线，定位异常窗口和可疑触发因素。
2. **有状态代码解释器工作区**：第一轮分析在 `/tmp/cubesandbox-incident-rca/state` 写入中间状态；第二轮在同一个沙箱中复用这些状态生成最终 RCA 产物。
3. **Matplotlib 中文渲染**：镜像内安装 `fonts-wqy-zenhei`，分析脚本配置 Matplotlib 使用 `WenQuanYi Zen Hei`，避免中文标题和坐标轴显示成方块。
4. **动态安装第三方包**：客户端在运行中的沙箱里执行 `pip3 install emoji humanize`，随后在第二轮分析中立即导入并参与报告生成。
5. **多文件输入和打包输出**：沙箱生成中文事故图、Markdown RCA 报告、SLO 汇总 CSV、部署关联 JSON、manifest JSON，并在沙箱内打成 tarball 供宿主机下载。

## 文件说明

- `Dockerfile`：构建可复用的 Python 数据科学沙箱镜像。
- `incident_metrics.csv`：`checkout-api` 的时间序列服务指标。
- `deployments.json`：用于关联分析的部署事件。
- `alerts.csv`：从监控系统导出的告警时间线。
- `runbook.json`：SLO 阈值和事故处理策略提示。
- `test_data_science.py`：端到端客户端，负责创建沙箱、上传文件、执行两轮分析、下载结果包，并验证 manifest。
- `env.example`：本地 CubeSandbox API/proxy 配置和模板 ID 占位符。

## 步骤 1：构建镜像

请在 Cube 节点运行时能够访问到该镜像的位置构建：

```bash
docker build -t cubesandbox-data-science:latest .
```

该镜像包含 Python、Pandas、Matplotlib、科学计算依赖和中文字体。由于面向代码解释器类负载，它会比最小镜像更大。

## 步骤 2：注册 CubeSandbox 模板

进入 CubeSandbox 开发虚拟机，或在已安装 `cubemastercli` 的节点上执行：

```bash
cubemastercli tpl create-from-image \
    --image               cubesandbox-data-science:latest \
    --writable-layer-size 2G \
    --expose-port         49983 \
    --probe               49983 \
    --probe-path          /health
```

记录命令返回的模板 ID，例如 `tpl-xxxxxxxxxxxxxxxxxxxxxxxx`。

## 步骤 3：配置客户端

安装客户端依赖：

```bash
pip3 install -r requirements.txt
```

复制 `.env` 并填入模板 ID：

```bash
cp env.example .env
```

本地 dev VM 场景下，`.env` 应类似：

```bash
E2B_API_URL="http://127.0.0.1:13000"
CUBE_REMOTE_PROXY_BASE="https://127.0.0.1:11443"
CUBE_TEMPLATE_ID="tpl-xxxxxxxxxxxxxxxxxxxxxxxx"
E2B_API_KEY=e2b_dummyapikeyforlocaltest
```

## 步骤 4：运行端到端示例

```bash
python3 test_data_science.py
```

脚本会依次完成：

1. 使用已注册模板启动沙箱。
2. 上传 `incident_metrics.csv`、`deployments.json`、`alerts.csv`、`runbook.json` 和两段生成的分析脚本。
3. 在沙箱内动态安装 `emoji` 和 `humanize`。
4. 执行第一轮异常检测，并把中间状态保存到沙箱工作区。
5. 执行第二轮 RCA 分析，复用第一轮状态、部署事件、告警和 runbook 策略。
6. 在沙箱内创建 `/tmp/cubesandbox-incident-rca-results.tar.gz`。
7. 下载并解压结果包到 `output/results/`。

预期解压结果：

- `事故分析图.png`：中文事故分析图，用于验证字体渲染正常。
- `incident_report.md`：Markdown RCA 报告，包含可疑触发因素和建议动作。
- `slo_summary.csv`：基线和事故峰值的 SLO 指标对比。
- `deployment_correlation.json`：部署事件与事故窗口的结构化关联结果。
- `manifest.json`：机器可读的产物清单和关键指标。
- `state/anomaly_windows.csv` 与 `state/baseline.json`：第一轮分析生成的中间状态，随结果包一起归档，便于审计。
