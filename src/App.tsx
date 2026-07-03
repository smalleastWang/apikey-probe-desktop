import { useEffect, useRef, useState } from "react";
import {
  CANCELED_MESSAGE,
  cancelProbe,
  chooseExportDirectory,
  exportMultiReportJson,
  exportMultiReportMarkdown,
  exportReportJson,
  exportReportMarkdown,
  inferProtocolType,
  listenProbeProgress,
  runMultiProtocolProbe,
  runProbe,
  saveReportFile,
} from "./lib/tauri";
import type {
  CheckResult,
  MultiProtocolProbeConfig,
  MultiProtocolProbeReport,
  ProbeConfig,
  ProbeProgress,
  ProbeReport,
  ProtocolType,
  StepStatus,
} from "./types";

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

const protocolInferenceNotes: Record<ProtocolType, string> = {
  "openai-compatible": "常见 GPT 4 / DeepSeek / Qwen / Moonshot / Kimi 等模型默认按 OpenAI-compatible 检测",
  "openai-responses": "GPT 5 系列模型默认按 OpenAI Responses API 检测",
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
};

const defaultSteps: ProbeProgress[] = [
  { step: "chat", label: "基础聊天", status: "PENDING", message: "等待检测" },
  { step: "tools", label: "Tools / Function Calling", status: "PENDING", message: "等待检测" },
  { step: "stream", label: "Stream 流式", status: "PENDING", message: "等待检测" },
  { step: "json_mode", label: "JSON Mode", status: "PENDING", message: "等待检测" },
  { step: "error_format", label: "错误格式", status: "PENDING", message: "等待检测" },
  { step: "risk", label: "逆向风险评分", status: "PENDING", message: "等待检测" },
];

type ProgressGroup = { protocol: ProtocolType | null; steps: ProbeProgress[] };

const MAX_TABS = 20;

type TabState = {
  id: string;
  seq: number;
  config: ProbeConfig;
  selectedProtocols: ProtocolType[];
  progressGroups: ProgressGroup[];
  report: ProbeReport | null;
  multiReport: MultiProtocolProbeReport | null;
  page: "form" | "report";
  running: boolean;
  canceling: boolean;
  error: string | null;
  notice: string | null;
  exportMessage: string | null;
  inferredProtocol: ProtocolType | null;
};

function createTab(seq: number): TabState {
  return {
    id:
      typeof crypto !== "undefined" && "randomUUID" in crypto
        ? crypto.randomUUID()
        : `tab-${Date.now()}-${Math.random().toString(16).slice(2)}`,
    seq,
    config: { ...initialConfig },
    selectedProtocols: ["openai-compatible"],
    progressGroups: [{ protocol: null, steps: defaultSteps }],
    report: null,
    multiReport: null,
    page: "form",
    running: false,
    canceling: false,
    error: null,
    notice: null,
    exportMessage: null,
    inferredProtocol: null,
  };
}

function App() {
  const [tabs, setTabs] = useState<TabState[]>(() => [createTab(1)]);
  const [activeTabId, setActiveTabId] = useState<string>(() => tabs[0].id);
  const [bulkMessage, setBulkMessage] = useState<string | null>(null);
  const [bulkError, setBulkError] = useState<string | null>(null);
  const [tabsOverflow, setTabsOverflow] = useState(false);

  const tabsRef = useRef(tabs);
  tabsRef.current = tabs;
  const seqRef = useRef(1);
  const listenersRef = useRef<Map<string, Promise<() => void>>>(new Map());
  const inferenceReqRef = useRef<Map<string, number>>(new Map());
  const activeTabRef = useRef<HTMLDivElement>(null);
  const tabBarRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef({ down: false, startX: 0, startScroll: 0, moved: false });

  const activeTab = tabs.find((tab) => tab.id === activeTabId) ?? tabs[0];

  useEffect(() => {
    activeTabRef.current?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [activeTabId]);

  useEffect(() => {
    const el = tabBarRef.current;
    if (!el) return;
    const measure = () => setTabsOverflow(el.scrollWidth - el.clientWidth > 1);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [tabs.length]);

  useEffect(() => {
    function onPointerMove(event: PointerEvent) {
      const state = dragRef.current;
      const el = tabBarRef.current;
      if (!state.down || !el) return;
      const dx = event.clientX - state.startX;
      if (!state.moved && Math.abs(dx) > 4) state.moved = true;
      if (state.moved) el.scrollLeft = state.startScroll - dx;
    }
    function onPointerUp() {
      dragRef.current.down = false;
    }
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", onPointerUp);
    window.addEventListener("pointercancel", onPointerUp);
    return () => {
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", onPointerUp);
      window.removeEventListener("pointercancel", onPointerUp);
    };
  }, []);

  useEffect(() => {
    const listeners = listenersRef.current;
    const currentIds = new Set(tabs.map((tab) => tab.id));

    tabs.forEach((tab) => {
      if (listeners.has(tab.id)) return;
      const pending = listenProbeProgress(tab.id, (progress) => {
        setTabs((prev) =>
          prev.map((item) =>
            item.id === tab.id
              ? { ...item, progressGroups: applyProgress(item.progressGroups, progress) }
              : item,
          ),
        );
      });
      listeners.set(tab.id, pending);
    });

    listeners.forEach((pending, id) => {
      if (!currentIds.has(id)) {
        pending.then((unlisten) => unlisten());
        listeners.delete(id);
      }
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tabs.map((tab) => tab.id).join("|")]);

  useEffect(() => {
    const listeners = listenersRef.current;
    return () => {
      listeners.forEach((pending) => pending.then((unlisten) => unlisten()));
      listeners.clear();
    };
  }, []);

  function updateTab(id: string, updater: (tab: TabState) => TabState) {
    setTabs((prev) => prev.map((tab) => (tab.id === id ? updater(tab) : tab)));
  }

  function addTab() {
    if (tabsRef.current.length >= MAX_TABS) return;
    seqRef.current += 1;
    const tab = createTab(seqRef.current);
    setTabs((prev) => [...prev, tab]);
    setActiveTabId(tab.id);
    setBulkMessage(null);
    setBulkError(null);
  }

  function closeTab(id: string) {
    const prev = tabsRef.current;
    if (prev.length <= 1) return;
    const index = prev.findIndex((tab) => tab.id === id);
    const next = prev.filter((tab) => tab.id !== id);
    setTabs(next);
    if (id === activeTabId) {
      const fallback = next[Math.max(0, index - 1)] ?? next[0];
      setActiveTabId(fallback.id);
    }
  }

  function handleTabBarPointerDown(event: React.PointerEvent<HTMLDivElement>) {
    const el = tabBarRef.current;
    if (!el) return;
    if ((event.target as HTMLElement).closest(".tab-close")) return;
    dragRef.current = {
      down: true,
      startX: event.clientX,
      startScroll: el.scrollLeft,
      moved: false,
    };
  }

  function handleTabBarClickCapture(event: React.MouseEvent<HTMLDivElement>) {
    if (dragRef.current.moved) {
      event.stopPropagation();
      event.preventDefault();
      dragRef.current.moved = false;
    }
  }

  async function runProbeForTab(id: string) {
    const current = tabsRef.current.find((tab) => tab.id === id);
    if (!current) return;

    const protocols = orderProtocols(current.selectedProtocols);
    if (protocols.length === 0) {
      updateTab(id, (tab) => ({ ...tab, error: "请至少选择一个协议类型" }));
      return;
    }

    updateTab(id, (tab) => ({
      ...tab,
      running: true,
      canceling: false,
      error: null,
      notice: null,
      exportMessage: null,
      report: null,
      multiReport: null,
      page: "form",
      progressGroups:
        protocols.length === 1
          ? [{ protocol: null, steps: defaultSteps }]
          : protocols.map((protocol) => ({ protocol, steps: defaultSteps })),
    }));

    // 确保本标签的进度监听器已就绪，避免开始检测后错过最早的进度事件
    await listenersRef.current.get(id);

    try {
      if (protocols.length === 1) {
        const nextReport = await runProbe(
          normalizeConfig({ ...current.config, protocolType: protocols[0] }),
          id,
        );
        updateTab(id, (tab) => ({ ...tab, report: nextReport }));
      } else {
        const nextReport = await runMultiProtocolProbe(
          normalizeMultiConfig(current.config, protocols),
          id,
        );
        updateTab(id, (tab) => ({ ...tab, multiReport: nextReport }));
      }
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      if (message.includes(CANCELED_MESSAGE)) {
        // 用户主动取消：不作为错误展示，仅给出中性提示。
        updateTab(id, (tab) => ({ ...tab, notice: "已取消本次检测" }));
      } else {
        updateTab(id, (tab) => ({ ...tab, error: message }));
      }
    } finally {
      updateTab(id, (tab) => ({ ...tab, running: false, canceling: false }));
    }
  }

  async function cancelProbeForTab(id: string) {
    const current = tabsRef.current.find((tab) => tab.id === id);
    if (!current || !current.running) return;
    updateTab(id, (tab) => ({ ...tab, canceling: true }));
    try {
      await cancelProbe(id);
    } catch {
      // 忽略取消命令本身的失败；检测任务会照常结束并清理状态。
    }
  }

  async function exportForTab(id: string, format: "json" | "markdown") {
    const current = tabsRef.current.find((tab) => tab.id === id);
    if (!current || (!current.report && !current.multiReport)) return;

    updateTab(id, (tab) => ({ ...tab, error: null, exportMessage: null }));

    const directory = await chooseExportDirectory();
    if (!directory) return;

    try {
      let content: string;
      let filename: string;
      const extension = format === "json" ? "json" : "md";

      if (current.multiReport) {
        content =
          format === "json"
            ? await exportMultiReportJson(current.multiReport)
            : await exportMultiReportMarkdown(current.multiReport);
        filename = `apikey-probe-${current.multiReport.model || "report"}-multi.${extension}`;
      } else if (current.report) {
        content =
          format === "json"
            ? await exportReportJson(current.report)
            : await exportReportMarkdown(current.report);
        filename = `apikey-probe-${current.report.config.model || "report"}.${extension}`;
      } else {
        return;
      }

      const savedPath = await saveReportFile(directory, filename, content);
      updateTab(id, (tab) => ({ ...tab, exportMessage: `已导出：${savedPath}` }));
    } catch (err) {
      updateTab(id, (tab) => ({
        ...tab,
        error: err instanceof Error ? err.message : String(err),
      }));
    }
  }

  async function downloadAllReports() {
    const finished = tabsRef.current.filter((tab) => tab.report || tab.multiReport);
    setBulkMessage(null);
    setBulkError(null);

    if (finished.length === 0) {
      setBulkError("暂无可下载的报告，请先完成检测");
      return;
    }

    const directory = await chooseExportDirectory();
    if (!directory) return;

    try {
      const saved: string[] = [];
      for (const tab of finished) {
        let content: string;
        let filename: string;

        if (tab.multiReport) {
          content = await exportMultiReportMarkdown(tab.multiReport);
          filename = `apikey-probe-${tab.seq}-${tab.multiReport.model || "report"}-multi.md`;
        } else if (tab.report) {
          content = await exportReportMarkdown(tab.report);
          filename = `apikey-probe-${tab.seq}-${tab.report.config.model || "report"}.md`;
        } else {
          continue;
        }

        const path = await saveReportFile(directory, filename, content);
        saved.push(path);
      }
      setBulkMessage(`已导出 ${saved.length} 份报告到：${directory}`);
    } catch (err) {
      setBulkError(err instanceof Error ? err.message : String(err));
    }
  }

  async function updateModelForTab(id: string, model: string) {
    const requestMap = inferenceReqRef.current;
    const nextRequest = (requestMap.get(id) ?? 0) + 1;
    requestMap.set(id, nextRequest);
    updateTab(id, (tab) => ({ ...tab, config: { ...tab.config, model } }));

    try {
      const inferred = await inferProtocolType(model);
      if (requestMap.get(id) !== nextRequest) return;
      updateTab(id, (tab) => ({
        ...tab,
        inferredProtocol: inferred,
        selectedProtocols: inferred ? [inferred] : tab.selectedProtocols,
      }));
    } catch {
      if (requestMap.get(id) !== nextRequest) return;
      updateTab(id, (tab) => ({ ...tab, inferredProtocol: null }));
    }
  }

  const hasAnyReport = tabs.some((tab) => tab.report || tab.multiReport);

  return (
    <main className="app">
      <section className="hero">
        <div className="hero-brand">
          <img className="hero-logo" src="/app-logo.png" alt="" />
          <div>
            <h1>Lingke AI渠道模型验证工具</h1>
          </div>
        </div>
        <div className="hero-actions">
          <button
            type="button"
            className="download-all"
            onClick={downloadAllReports}
            disabled={!hasAnyReport}
          >
            下载全部报告
          </button>
          <button
            type="button"
            className="add-tab"
            onClick={addTab}
            disabled={tabs.length >= MAX_TABS}
            title={tabs.length >= MAX_TABS ? `最多 ${MAX_TABS} 个标签页` : "新建检测标签页"}
            aria-label="新建检测标签页"
          >
            ＋
          </button>
        </div>
      </section>

      {bulkMessage && <div className="success-box">{bulkMessage}</div>}
      {bulkError && <div className="error-box">{bulkError}</div>}

      <div
        className={`tab-bar ${tabsOverflow ? "draggable" : ""}`}
        role="tablist"
        ref={tabBarRef}
        onPointerDown={handleTabBarPointerDown}
        onClickCapture={handleTabBarClickCapture}
      >
        {tabs.map((tab) => (
          <div
            key={tab.id}
            ref={tab.id === activeTabId ? activeTabRef : undefined}
            role="tab"
            aria-selected={tab.id === activeTabId}
            className={`tab ${tab.id === activeTabId ? "active" : ""}`}
            onClick={() => setActiveTabId(tab.id)}
          >
            <span className={`tab-dot ${tabDotClass(tab)}`} />
            <span className="tab-title">{tabTitle(tab)}</span>
            {tabs.length > 1 && (
              <button
                type="button"
                className="tab-close"
                title="关闭标签页"
                aria-label="关闭标签页"
                onClick={(event) => {
                  event.stopPropagation();
                  closeTab(tab.id);
                }}
              >
                ×
              </button>
            )}
          </div>
        ))}
      </div>

      <div
        className={`tab-body ${tabsOverflow ? "tabs-overflow" : ""}`}
        key={activeTab.id}
      >
        {activeTab.page === "report" ? (
          <TabReportDetail
            tab={activeTab}
            onBack={() => updateTab(activeTab.id, (tab) => ({ ...tab, page: "form" }))}
          />
        ) : (
          <TabForm
            tab={activeTab}
            onUpdateConfig={(key, value) =>
              updateTab(activeTab.id, (tab) => ({
                ...tab,
                config: { ...tab.config, [key]: value },
              }))
            }
            onToggleProtocol={(value) =>
              updateTab(activeTab.id, (tab) => ({
                ...tab,
                selectedProtocols: tab.selectedProtocols.includes(value)
                  ? tab.selectedProtocols.filter((item) => item !== value)
                  : [...tab.selectedProtocols, value],
              }))
            }
            onUpdateModel={(model) => void updateModelForTab(activeTab.id, model)}
            onRun={() => void runProbeForTab(activeTab.id)}
            onCancel={() => void cancelProbeForTab(activeTab.id)}
            onExport={(format) => void exportForTab(activeTab.id, format)}
            onOpenReport={() =>
              updateTab(activeTab.id, (tab) => ({ ...tab, page: "report" }))
            }
          />
        )}
      </div>
    </main>
  );
}

function TabForm({
  tab,
  onUpdateConfig,
  onToggleProtocol,
  onUpdateModel,
  onRun,
  onCancel,
  onExport,
  onOpenReport,
}: {
  tab: TabState;
  onUpdateConfig: <K extends keyof ProbeConfig>(key: K, value: ProbeConfig[K]) => void;
  onToggleProtocol: (value: ProtocolType) => void;
  onUpdateModel: (model: string) => void;
  onRun: () => void;
  onCancel: () => void;
  onExport: (format: "json" | "markdown") => void;
  onOpenReport: () => void;
}) {
  const { config, selectedProtocols, progressGroups, report, multiReport, running, canceling, error, notice, exportMessage, inferredProtocol } = tab;
  const primaryProtocol = orderProtocols(selectedProtocols)[0] ?? "openai-compatible";
  const activeConclusion = report?.conclusion ?? multiReport?.conclusion ?? null;
  const canRun =
    config.baseUrl.trim().length > 0 &&
    config.apiKey.trim().length > 0 &&
    config.model.trim().length > 0 &&
    selectedProtocols.length > 0 &&
    !running;

  return (
    <div className="layout">
      <section className="card">
        <div className="card-header">
          <h2>检测配置</h2>
        </div>

        <div className="form-grid">
          <Field label="Base URL" required>
            <input
              placeholder={
                protocolOptions.find((item) => item.value === primaryProtocol)?.placeholder
              }
              value={config.baseUrl}
              onChange={(event) => onUpdateConfig("baseUrl", event.target.value)}
            />
          </Field>
          <Field label="API Key" required>
            <input
              type="password"
              placeholder="sk-..."
              value={config.apiKey}
              onChange={(event) => onUpdateConfig("apiKey", event.target.value)}
            />
          </Field>
          <Field label="模型名" required>
            <input
              list="model-options"
              placeholder="选择常用模型或手动输入，例如 vendor-model-2026"
              value={config.model}
              onChange={(event) => onUpdateModel(event.target.value)}
            />
            <datalist id="model-options">
              {modelOptions.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label}
                </option>
              ))}
            </datalist>
          </Field>
          <Field label="协议类型（可多选）" required>
            <ProtocolMultiSelect
              options={protocolOptions}
              selected={selectedProtocols}
              onToggle={onToggleProtocol}
            />
            {inferredProtocol && (
              <p className="field-help">
                已根据模型名推测：{protocolLabel(inferredProtocol)}。
                {protocolInferenceNotes[inferredProtocol]}
              </p>
            )}
          </Field>
          <Field label="供应商名称">
            <input
              placeholder="仅用于报告归档"
              value={config.providerName}
              onChange={(event) => onUpdateConfig("providerName", event.target.value)}
            />
          </Field>
          <Field label="代理地址">
            <input
              placeholder="http://127.0.0.1:7890"
              value={config.proxyUrl}
              onChange={(event) => onUpdateConfig("proxyUrl", event.target.value)}
            />
          </Field>
          <Field label="备注">
            <textarea
              placeholder="供应商承诺、测试背景、上下游信息等"
              value={config.note}
              onChange={(event) => onUpdateConfig("note", event.target.value)}
            />
          </Field>
        </div>

        <div className="run-actions">
          <button className="primary" disabled={!canRun} onClick={onRun}>
            {running ? "检测中..." : "开始验货"}
          </button>
          {running && (
            <button
              type="button"
              className="cancel-run"
              disabled={canceling}
              onClick={onCancel}
            >
              {canceling ? "取消中..." : "取消检测"}
            </button>
          )}
        </div>

        {error && <div className="error-box">{error}</div>}
        {notice && <div className="notice-box">{notice}</div>}
        {exportMessage && <div className="success-box">{exportMessage}</div>}
      </section>

      <section className="stack">
        <div className="card">
          <div className="card-header">
            <h2>检测进度</h2>
            <div className={`conclusion-pill ${activeConclusion?.toLowerCase() || "idle"}`}>
              {activeConclusion ?? (running ? "RUNNING" : "READY")}
            </div>
          </div>
          <div className="steps-groups">
            {progressGroups.map((group) => (
              <div className="steps-group" key={group.protocol ?? "single"}>
                {group.protocol && (
                  <h3 className="steps-group-title">{protocolLabel(group.protocol)}</h3>
                )}
                <div className="steps">
                  {group.steps.map((step) => (
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
            ))}
          </div>
        </div>

        {report && (
          <div className="card">
            <div className="card-header">
              <h2>验货报告</h2>
              <div className="actions">
                <button onClick={onOpenReport}>查看完整报告</button>
                <button onClick={() => onExport("json")}>导出 JSON</button>
                <button onClick={() => onExport("markdown")}>导出 Markdown</button>
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

        {multiReport && (
          <div className="card">
            <div className="card-header">
              <h2>多协议验货报告</h2>
              <div className="actions">
                <button onClick={onOpenReport}>查看完整报告</button>
                <button onClick={() => onExport("json")}>导出 JSON</button>
                <button onClick={() => onExport("markdown")}>导出 Markdown</button>
              </div>
            </div>

            <div className={`report-summary ${multiReport.conclusion.toLowerCase()}`}>
              <strong>{multiReport.conclusion}</strong>
              <p>{multiReport.conclusionText}</p>
            </div>

            {multiReport.bestProtocol && (
              <p className="field-help">
                表现最佳协议：{protocolLabel(multiReport.bestProtocol)}
              </p>
            )}

            <div className="protocol-pills">
              {multiReport.results.map((result) => (
                <div className="protocol-pill" key={result.config.protocolType}>
                  <span>{protocolLabel(result.config.protocolType as ProtocolType)}</span>
                  <strong className={`status ${result.conclusion.toLowerCase()}`}>
                    {result.conclusion}
                  </strong>
                </div>
              ))}
            </div>
          </div>
        )}
      </section>
    </div>
  );
}

function TabReportDetail({ tab, onBack }: { tab: TabState; onBack: () => void }) {
  if (tab.multiReport) {
    return <MultiReportDetail report={tab.multiReport} onBack={onBack} />;
  }
  if (tab.report) {
    return <ReportDetail report={tab.report} onBack={onBack} />;
  }
  return null;
}

function ReportDetail({
  report,
  onBack,
}: {
  report: ProbeReport;
  onBack: () => void;
}) {
  return (
    <>
      <section className="detail-head">
        <div>
          <p className="eyebrow">Probe Report</p>
          <h2>验货报告详情</h2>
          <p className="subtle">
            展示本次探针的完整证据、原始响应预览和逆向风险信号。
          </p>
        </div>
        <button onClick={onBack}>返回当前标签</button>
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

        <ChecksList checks={report.checks} />

        <RiskSignals report={report} />
      </section>
    </>
  );
}

function MultiReportDetail({
  report,
  onBack,
}: {
  report: MultiProtocolProbeReport;
  onBack: () => void;
}) {
  return (
    <>
      <section className="detail-head">
        <div>
          <p className="eyebrow">Probe Report</p>
          <h2>多协议验货报告详情</h2>
          <p className="subtle">
            对模型 {report.model} 在多个协议下的探针结果进行汇总对比。
          </p>
        </div>
        <button onClick={onBack}>返回当前标签</button>
      </section>

      <section className="card">
        <div className={`report-summary ${report.conclusion.toLowerCase()}`}>
          <strong>{report.conclusion}</strong>
          <p>{report.conclusionText}</p>
        </div>
        {report.bestProtocol && (
          <p className="field-help">表现最佳协议：{protocolLabel(report.bestProtocol)}</p>
        )}
      </section>

      {report.results.map((result) => (
        <section className="card" key={result.config.protocolType}>
          <div className="card-header">
            <h2>{protocolLabel(result.config.protocolType as ProtocolType)}</h2>
            <span className={`status ${result.conclusion.toLowerCase()}`}>
              {result.conclusion}
            </span>
          </div>

          <div className="risk-box">
            <div>
              <span>逆向风险分</span>
              <strong>{result.risk.score}</strong>
            </div>
            <div>
              <span>风险等级</span>
              <strong>{result.risk.level}</strong>
            </div>
          </div>

          <ChecksList checks={result.checks} />

          <RiskSignals report={result} />
        </section>
      ))}
    </>
  );
}

function ChecksList({ checks }: { checks: CheckResult[] }) {
  return (
    <div className="checks">
      {checks.map((check) => (
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
  );
}

function RiskSignals({ report }: { report: ProbeReport }) {
  if (report.risk.signals.length === 0) return null;

  return (
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
  );
}

function ProtocolMultiSelect({
  options,
  selected,
  onToggle,
}: {
  options: readonly { value: ProtocolType; label: string }[];
  selected: ProtocolType[];
  onToggle: (value: ProtocolType) => void;
}) {
  const [open, setOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    function handleClick(event: MouseEvent) {
      if (containerRef.current && !containerRef.current.contains(event.target as Node)) {
        setOpen(false);
      }
    }
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, [open]);

  const selectedOptions = options.filter((option) => selected.includes(option.value));

  return (
    <div className={`multiselect ${open ? "open" : ""}`} ref={containerRef}>
      <button
        type="button"
        className="multiselect-trigger"
        onClick={() => setOpen((value) => !value)}
      >
        <span className="multiselect-values">
          {selectedOptions.length === 0 ? (
            <span className="multiselect-placeholder">请选择协议类型（可多选）</span>
          ) : (
            selectedOptions.map((option) => (
              <span className="multiselect-chip" key={option.value}>
                {option.label}
              </span>
            ))
          )}
        </span>
        <span className="multiselect-caret">▾</span>
      </button>
      {open && (
        <div className="multiselect-panel">
          {options.map((option) => (
            <label key={option.value} className="multiselect-option">
              <input
                type="checkbox"
                checked={selected.includes(option.value)}
                onChange={() => onToggle(option.value)}
              />
              <span>{option.label}</span>
            </label>
          ))}
        </div>
      )}
    </div>
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

function StatusBadge({ status }: { status: StepStatus | CheckResult["status"] }) {
  return <span className={`status ${status.toLowerCase()}`}>{status}</span>;
}

function protocolLabel(value: ProtocolType) {
  return protocolOptions.find((item) => item.value === value)?.label ?? value;
}

function tabTitle(tab: TabState) {
  return `检测 ${tab.seq}`;
}

function tabDotClass(tab: TabState) {
  if (tab.running) return "running";
  const conclusion = tab.report?.conclusion ?? tab.multiReport?.conclusion ?? null;
  return conclusion ? conclusion.toLowerCase() : "idle";
}

function applyProgress(groups: ProgressGroup[], progress: ProbeProgress): ProgressGroup[] {
  return groups.map((group) => {
    const groupProtocol = group.protocol ?? undefined;
    if ((progress.protocol as ProtocolType | undefined) !== groupProtocol) {
      return group;
    }
    return {
      ...group,
      steps: group.steps.map((step) =>
        step.step === progress.step ? { ...progress } : step,
      ),
    };
  });
}

function orderProtocols(list: ProtocolType[]): ProtocolType[] {
  return protocolOptions
    .map((option) => option.value)
    .filter((value) => list.includes(value));
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

function normalizeMultiConfig(
  config: ProbeConfig,
  protocols: ProtocolType[],
): MultiProtocolProbeConfig {
  return {
    baseUrl: config.baseUrl.trim(),
    apiKey: config.apiKey.trim(),
    model: config.model.trim(),
    protocolTypes: protocols,
    providerName: optional(config.providerName),
    note: optional(config.note),
    proxyUrl: optional(config.proxyUrl),
  };
}

function optional(value?: string) {
  const trimmed = value?.trim();
  return trimmed ? trimmed : undefined;
}

export default App;
