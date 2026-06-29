import { useEffect, useMemo, useState } from "react";
import {
  chooseExportDirectory,
  exportReportJson,
  exportReportMarkdown,
  listenProbeProgress,
  runProbe,
  saveReportFile,
} from "./lib/tauri";
import type { ProbeConfig, ProbeProgress, ProbeReport, StepStatus } from "./types";

const protocolOptions = [
  {
    value: "openai-compatible",
    label: "OpenAI-compatible Chat Completions",
    placeholder: "https://api.example.com 或 https://api.example.com/v1",
  },
  {
    value: "openai-responses",
    label: "OpenAI Responses API",
    placeholder: "https://api.openai.com 或兼容 /v1/responses 的地址",
  },
  {
    value: "anthropic-messages",
    label: "Anthropic Messages API",
    placeholder: "https://api.anthropic.com 或兼容 /v1/messages 的地址",
  },
  {
    value: "google-gemini",
    label: "Google Gemini API",
    placeholder: "https://generativelanguage.googleapis.com 或兼容 v1beta 地址",
  },
] as const;

const modelOptions = [
  { value: "gpt-4o", label: "OpenAI: gpt-4o" },
  { value: "gpt-4o-mini", label: "OpenAI: gpt-4o-mini" },
  { value: "gpt-4.1", label: "OpenAI: gpt-4.1" },
  { value: "gpt-4.1-mini", label: "OpenAI: gpt-4.1-mini" },
  { value: "claude-3-5-sonnet-latest", label: "Anthropic: claude-3-5-sonnet-latest" },
  { value: "claude-3-7-sonnet-latest", label: "Anthropic: claude-3-7-sonnet-latest" },
  { value: "gemini-1.5-pro", label: "Gemini: gemini-1.5-pro" },
  { value: "gemini-1.5-flash", label: "Gemini: gemini-1.5-flash" },
  { value: "deepseek-chat", label: "DeepSeek: deepseek-chat" },
  { value: "deepseek-reasoner", label: "DeepSeek: deepseek-reasoner" },
  { value: "qwen-plus", label: "Qwen: qwen-plus" },
  { value: "qwen-max", label: "Qwen: qwen-max" },
  { value: "moonshot-v1-8k", label: "Moonshot: moonshot-v1-8k" },
] as const;

const protocolInferenceNotes: Record<ProbeConfig["protocolType"], string> = {
  "openai-compatible": "常见 GPT / DeepSeek / Qwen / Moonshot / Kimi 等模型默认按 OpenAI-compatible 检测",
  "openai-responses": "模型名通常无法单独判断 Responses API，需要按上游文档手动选择",
  "anthropic-messages": "Claude 系列模型默认按 Anthropic Messages API 检测",
  "google-gemini": "Gemini 系列模型默认按 Google Gemini API 检测",
};

const initialConfig: ProbeConfig = {
  baseUrl: "",
  apiKey: "",
  model: "",
  protocolType: "openai-compatible",
  providerName: "",
  note: "",
  proxyUrl: "",
  saveApiKey: false,
};

const defaultSteps: ProbeProgress[] = [
  { step: "chat", label: "基础聊天", status: "PENDING", message: "等待检测" },
  { step: "tools", label: "Tools / Function Calling", status: "PENDING", message: "等待检测" },
  { step: "stream", label: "Stream 流式", status: "PENDING", message: "等待检测" },
  { step: "json_mode", label: "JSON Mode", status: "PENDING", message: "等待检测" },
  { step: "error_format", label: "错误格式", status: "PENDING", message: "等待检测" },
  { step: "risk", label: "逆向风险评分", status: "PENDING", message: "等待检测" },
];

function App() {
  const [config, setConfig] = useState<ProbeConfig>(initialConfig);
  const [steps, setSteps] = useState<ProbeProgress[]>(defaultSteps);
  const [report, setReport] = useState<ProbeReport | null>(null);
  const [page, setPage] = useState<"form" | "report">("form");
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [exportMessage, setExportMessage] = useState<string | null>(null);
  const inferredProtocol = inferProtocolType(config.model);

  useEffect(() => {
    const unlistenPromise = listenProbeProgress((progress) => {
      setSteps((current) =>
        current.map((step) => (step.step === progress.step ? progress : step)),
      );
    });

    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);

  const canRun = useMemo(
    () =>
      config.baseUrl.trim().length > 0 &&
      config.apiKey.trim().length > 0 &&
      config.model.trim().length > 0 &&
      !running,
    [config, running],
  );

  async function handleRun() {
    setRunning(true);
    setError(null);
    setExportMessage(null);
    setReport(null);
    setPage("form");
    setSteps(defaultSteps);

    try {
      const nextReport = await runProbe(normalizeConfig(config));
      setReport(nextReport);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setRunning(false);
    }
  }

  async function handleExport(format: "json" | "markdown") {
    if (!report) return;
    setError(null);
    setExportMessage(null);

    const directory = await chooseExportDirectory();
    if (!directory) return;

    const content =
      format === "json"
        ? await exportReportJson(report)
        : await exportReportMarkdown(report);
    const filename = `apikey-probe-${report.config.model || "report"}.${format === "json" ? "json" : "md"}`;

    try {
      const savedPath = await saveReportFile(directory, filename, content);
      setExportMessage(`已导出：${savedPath}`);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  if (page === "report" && report) {
    return <ReportDetail report={report} onBack={() => setPage("form")} />;
  }

  return (
    <main className="app">
      <section className="hero">
        <div className="hero-brand">
          <img className="hero-logo" src="/app-logo.png" alt="" />
          <div>
            <p className="eyebrow">Local Desktop Probe</p>
            <h1>上游 API Key / 模型验货工具</h1>
            <p className="subtle">
              本地填写 Base URL、API Key、模型名后，由 Rust 后端请求上游 API，
              检测 OpenAI Chat、OpenAI Responses、Anthropic Messages、Gemini 的基础能力、
              满血 tools、stream、JSON mode 和疑似逆向风险。
            </p>
          </div>
        </div>
        <div className={`conclusion-pill ${report?.conclusion.toLowerCase() || "idle"}`}>
          {report ? report.conclusion : running ? "RUNNING" : "READY"}
        </div>
      </section>

      <div className="layout">
        <section className="card">
          <div className="card-header">
            <h2>检测配置</h2>
            <span>API Key 默认只在本次检测内使用</span>
          </div>

          <div className="form-grid">
            <Field label="Base URL" required>
              <input
                placeholder={
                  protocolOptions.find((item) => item.value === config.protocolType)?.placeholder
                }
                value={config.baseUrl}
                onChange={(event) => updateConfig("baseUrl", event.target.value)}
              />
            </Field>
            <Field label="API Key" required>
              <input
                type="password"
                placeholder="sk-..."
                value={config.apiKey}
                onChange={(event) => updateConfig("apiKey", event.target.value)}
              />
            </Field>
            <Field label="模型名" required>
              <input
                list="model-options"
                placeholder="选择常用模型或手动输入，例如 vendor-model-2026"
                value={config.model}
                onChange={(event) => updateConfig("model", event.target.value)}
              />
              <datalist id="model-options">
                {modelOptions.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </datalist>
            </Field>
            <Field label="协议类型" required>
              <select
                value={config.protocolType}
                onChange={(event) => updateConfig("protocolType", event.target.value as ProbeConfig["protocolType"])}
              >
                {protocolOptions.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
              {inferredProtocol && (
                <p className="field-help">
                  已根据模型名推测：{protocolOptions.find((item) => item.value === inferredProtocol)?.label}。
                  {protocolInferenceNotes[inferredProtocol]}
                </p>
              )}
            </Field>
            <Field label="供应商名称">
              <input
                placeholder="仅用于报告归档"
                value={config.providerName}
                onChange={(event) => updateConfig("providerName", event.target.value)}
              />
            </Field>
            <Field label="代理地址">
              <input
                placeholder="http://127.0.0.1:7890"
                value={config.proxyUrl}
                onChange={(event) => updateConfig("proxyUrl", event.target.value)}
              />
            </Field>
            <Field label="备注">
              <textarea
                placeholder="供应商承诺、测试背景、上下游信息等"
                value={config.note}
                onChange={(event) => updateConfig("note", event.target.value)}
              />
            </Field>
          </div>

          <label className="checkbox-row">
            <input
              type="checkbox"
              checked={config.saveApiKey}
              onChange={(event) => updateConfig("saveApiKey", event.target.checked)}
            />
            是否保存 API Key（MVP 暂不落盘，仅保留字段）
          </label>

          <button className="primary" disabled={!canRun} onClick={handleRun}>
            {running ? "检测中..." : "开始验货"}
          </button>

          {error && <div className="error-box">{error}</div>}
          {exportMessage && <div className="success-box">{exportMessage}</div>}
        </section>

        <section className="stack">
          <div className="card">
            <div className="card-header">
              <h2>检测进度</h2>
              <span>Rust 后端通过 Tauri event 推送</span>
            </div>
            <div className="steps">
              {steps.map((step) => (
                <div className="step" key={step.step}>
                  <StatusBadge status={step.status} />
                  <div>
                    <strong>{step.label}</strong>
                    <p>{step.message}</p>
                  </div>
                </div>
              ))}
            </div>
          </div>

          {report && (
            <div className="card">
              <div className="card-header">
                <h2>验货报告</h2>
                <div className="actions">
                  <button onClick={() => setPage("report")}>查看完整报告</button>
                  <button onClick={() => handleExport("json")}>导出 JSON</button>
                  <button onClick={() => handleExport("markdown")}>导出 Markdown</button>
                </div>
              </div>

              <div className={`report-summary ${report.conclusion.toLowerCase()}`}>
                <strong>{report.conclusion}</strong>
                <p>{report.conclusionText}</p>
              </div>

              <div className="risk-box">
                <div>
                  <span>逆向风险分</span>
                  <strong>{report.risk.score}</strong>
                </div>
                <div>
                  <span>风险等级</span>
                  <strong>{report.risk.level}</strong>
                </div>
              </div>
            </div>
          )}
        </section>
      </div>
    </main>
  );

  function updateConfig<K extends keyof ProbeConfig>(key: K, value: ProbeConfig[K]) {
    setConfig((current) => {
      if (key !== "model" || typeof value !== "string") {
        return { ...current, [key]: value };
      }

      const inferred = inferProtocolType(value);
      return {
        ...current,
        model: value,
        protocolType: inferred ?? current.protocolType,
      };
    });
  }
}

function ReportDetail({
  report,
  onBack,
}: {
  report: ProbeReport;
  onBack: () => void;
}) {
  return (
    <main className="app">
      <section className="hero">
        <div>
          <p className="eyebrow">Probe Report</p>
          <h1>验货报告详情</h1>
          <p className="subtle">
            展示本次探针的完整证据、原始响应预览和逆向风险信号。
          </p>
        </div>
        <button onClick={onBack}>返回检测页</button>
      </section>

      <section className="card">
        <div className={`report-summary ${report.conclusion.toLowerCase()}`}>
          <strong>{report.conclusion}</strong>
          <p>{report.conclusionText}</p>
        </div>

        <div className="risk-box">
          <div>
            <span>逆向风险分</span>
            <strong>{report.risk.score}</strong>
          </div>
          <div>
            <span>风险等级</span>
            <strong>{report.risk.level}</strong>
          </div>
        </div>

        <div className="checks">
          {report.checks.map((check) => (
            <details className="check" key={check.key} open={check.status !== "PASS"}>
              <summary>
                <StatusBadge status={check.status} />
                <span>{check.label}</span>
                <strong>{check.summary}</strong>
              </summary>
              {check.evidence.length > 0 && (
                <ul>
                  {check.evidence.map((item) => (
                    <li key={item}>{item}</li>
                  ))}
                </ul>
              )}
              {check.rawPreview && <pre>{check.rawPreview}</pre>}
            </details>
          ))}
        </div>

        {report.risk.signals.length > 0 && (
          <div className="risk-signals">
            <h3>风险信号</h3>
            {report.risk.signals.map((signal) => (
              <div key={signal.key}>
                <strong>{signal.label}</strong>
                <span>
                  {signal.severity} +{signal.score}
                </span>
                <p>{signal.evidence}</p>
              </div>
            ))}
          </div>
        )}
      </section>
    </main>
  );
}

function Field({
  label,
  required,
  children,
}: {
  label: string;
  required?: boolean;
  children: React.ReactNode;
}) {
  return (
    <label className="field">
      <span>
        {label}
        {required && <b>*</b>}
      </span>
      {children}
    </label>
  );
}

function StatusBadge({ status }: { status: StepStatus }) {
  return <span className={`status ${status.toLowerCase()}`}>{status}</span>;
}

function normalizeConfig(config: ProbeConfig): ProbeConfig {
  return {
    ...config,
    baseUrl: config.baseUrl.trim(),
    apiKey: config.apiKey.trim(),
    model: config.model.trim(),
    providerName: optional(config.providerName),
    note: optional(config.note),
    proxyUrl: optional(config.proxyUrl),
  };
}

function optional(value?: string) {
  const trimmed = value?.trim();
  return trimmed ? trimmed : undefined;
}

function inferProtocolType(model: string): ProbeConfig["protocolType"] | null {
  const normalized = model.trim().toLowerCase();
  if (!normalized) return null;

  if (/(^|[/_-])claude($|[\d._-])/.test(normalized) || normalized.includes("anthropic")) {
    return "anthropic-messages";
  }

  if (/(^|[/_-])gemini($|[\d._-])/.test(normalized) || normalized.includes("models/gemini")) {
    return "google-gemini";
  }

  if (
    /(^|[/_-])(gpt|o[134]|chatgpt|deepseek|qwen|qwq|moonshot|kimi|glm|doubao|yi|llama|mistral)($|[\d._:-])/.test(
      normalized,
    )
  ) {
    return "openai-compatible";
  }

  return null;
}

export default App;
