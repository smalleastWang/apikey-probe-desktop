import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type {
  MultiProtocolProbeConfig,
  MultiProtocolProbeReport,
  ProbeConfig,
  ProbeProgress,
  ProbeReport,
} from "../types";

export function runProbe(config: ProbeConfig, sessionId: string) {
  return invoke<ProbeReport>("run_openai_compatible_probe", { config, sessionId });
}

export function runMultiProtocolProbe(config: MultiProtocolProbeConfig, sessionId: string) {
  return invoke<MultiProtocolProbeReport>("run_multi_protocol_probe", { config, sessionId });
}

/** 后端主动取消返回的标记，前端据此把结果显示为"已取消"而非错误。 */
export const CANCELED_MESSAGE = "PROBE_CANCELED";

export function cancelProbe(sessionId: string) {
  return invoke<void>("cancel_probe", { sessionId });
}

export function exportReportJson(report: ProbeReport) {
  return invoke<string>("export_report_json", { report });
}

export function exportReportMarkdown(report: ProbeReport) {
  return invoke<string>("export_report_markdown", { report });
}

export function exportMultiReportJson(report: MultiProtocolProbeReport) {
  return invoke<string>("export_multi_report_json", { report });
}

export function exportMultiReportMarkdown(report: MultiProtocolProbeReport) {
  return invoke<string>("export_multi_report_markdown", { report });
}

export function inferProtocolType(model: string) {
  return invoke<ProbeConfig["protocolType"] | null>("infer_protocol_type", { model });
}

export async function chooseExportDirectory() {
  const selected = await open({
    directory: true,
    multiple: false,
    title: "选择报告导出文件夹",
  });

  return typeof selected === "string" ? selected : null;
}

export function saveReportFile(directory: string, filename: string, content: string) {
  return invoke<string>("save_report_file", { directory, filename, content });
}

export function listenProbeProgress(
  sessionId: string,
  callback: (progress: ProbeProgress) => void,
) {
  const eventName = sessionId ? `probe-progress:${sessionId}` : "probe-progress";
  return listen<ProbeProgress>(eventName, (event) => callback(event.payload));
}
