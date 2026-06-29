import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type { ProbeConfig, ProbeProgress, ProbeReport } from "../types";

export function runProbe(config: ProbeConfig) {
  return invoke<ProbeReport>("run_openai_compatible_probe", { config });
}

export function exportReportJson(report: ProbeReport) {
  return invoke<string>("export_report_json", { report });
}

export function exportReportMarkdown(report: ProbeReport) {
  return invoke<string>("export_report_markdown", { report });
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

export function listenProbeProgress(callback: (progress: ProbeProgress) => void) {
  return listen<ProbeProgress>("probe-progress", (event) => callback(event.payload));
}
