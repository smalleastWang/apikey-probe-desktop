export type ProtocolType =
  | "openai-compatible"
  | "openai-responses"
  | "anthropic-messages"
  | "google-gemini";

export type ProbeConfig = {
  baseUrl: string;
  apiKey: string;
  model: string;
  protocolType: ProtocolType;
  providerName?: string;
  note?: string;
  proxyUrl?: string;
  saveApiKey: boolean;
};

export type StepStatus = "PENDING" | "RUNNING" | "PASS" | "WARN" | "FAIL";
export type CheckStatus = "PASS" | "WARN" | "FAIL";
export type OverallConclusion = "PASS" | "WARN" | "FAIL";
export type RiskLevel = "LOW" | "MEDIUM" | "HIGH";
export type RiskSeverity = "LOW" | "MEDIUM" | "HIGH";

export type ProbeProgress = {
  step: string;
  label: string;
  status: StepStatus;
  message: string;
};

export type CheckResult = {
  key: string;
  label: string;
  status: CheckStatus;
  summary: string;
  evidence: string[];
  rawPreview?: string;
};

export type RiskSignal = {
  key: string;
  label: string;
  severity: RiskSeverity;
  score: number;
  evidence: string;
};

export type RiskAssessment = {
  score: number;
  level: RiskLevel;
  signals: RiskSignal[];
};

export type ProbeReport = {
  generatedAt: string;
  config: Omit<ProbeConfig, "apiKey"> & { apiKey: string };
  conclusion: OverallConclusion;
  conclusionText: string;
  checks: CheckResult[];
  risk: RiskAssessment;
};
