import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// session-butler insights::Report 미러 (serde 필드명 = snake_case)
export interface Overview {
  sessions: number;
  total_tokens: number;
  total_tool_calls: number;
  total_file_changes: number;
  distinct_projects: number;
  distinct_tools: number;
  archived: number;
  date_from: string | null;
  date_to: string | null;
}
export interface ToolStat { tool: string; calls: number; }
export interface ProjectStat { repo: string; sessions: number; tokens: number; }
export interface TimeBucket {
  label: string; sessions: number; tokens: number;
  top_skill: string | null; top_skill_calls: number; top_words: string[];
}
export interface WeekdayStat { weekday: string; weekday_index: number; sessions: number; }
export interface WordStat { word: string; count: number; }
export interface WordSection { category: string; words: WordStat[]; }
export interface SessionStat {
  session_id: string; date: string | null;
  tokens: number; tool_calls: number; prompt: string | null;
}
export interface Report {
  window_days: number;
  granularity: string;
  words_source: string;
  words_fallback: boolean;
  overview: Overview;
  top_tools: ToolStat[];
  least_used_tools: ToolStat[];
  top_projects: ProjectStat[];
  time_buckets: TimeBucket[];
  activity_by_weekday: WeekdayStat[];
  peak_hour: number | null;
  top_words: WordSection[];
  token_leaders: SessionStat[];
}

export interface ScanProgress {
  kind: "bar" | "spinner" | "inc" | "finish" | "warn";
  n?: number;
  len?: number;
  msg?: string;
}

export function getInsights(days: number, top: number, by: string, words: string) {
  return invoke<Report | null>("get_insights", { days, top, by, words });
}

export interface ScanSummary { sessions: number; }

/** scan 실행 + "scan-progress" 이벤트 수신. onProgress 로 진행률 알림. */
export async function runScan(onProgress: (p: ScanProgress) => void): Promise<ScanSummary> {
  let unlisten: UnlistenFn | undefined;
  try {
    unlisten = await listen<ScanProgress>("scan-progress", (e) => onProgress(e.payload));
  } catch {
    // 이벤트 수신 불가 환경에서는 무시
  }
  try {
    return await invoke<ScanSummary>("scan");
  } finally {
    unlisten?.();
  }
}

// ---- Phase 2: archive / restore / compact ----

/** 진행률 이벤트 채널명(커맨드별). */
export const PROGRESS_EVENTS = {
  scan: "scan-progress",
  archive: "archive-progress",
  restore: "restore-progress",
  compact: "compact-progress",
  scanSensitive: "scan-sensitive-progress",
} as const;

export interface ArchivedRow {
  session_id: string;
  path: string;
  date: string | null;
  compressed_path: string;
  checksum_sha256: string;
}
export interface SensitiveFile {
  path: string;
  date: string | null;
  size_bytes: number;
  patterns: string[];
}
export interface ArchiveSummary {
  archived: number; skipped: number; total_original: number; total_compressed: number;
}
export interface RestoreSummary { restored: number; }
export interface CompactSummary { moved: number; skipped: number; total: number; }

/** 진행률 이벤트를 수신하며 명령 실행. */
async function runWithProgress<T>(
  event: string,
  cmd: string,
  args: Record<string, unknown>,
  onProgress?: (p: ScanProgress) => void,
): Promise<T> {
  let unlisten: UnlistenFn | undefined;
  if (onProgress) {
    try {
      unlisten = await listen<ScanProgress>(event, (e) => onProgress(e.payload));
    } catch {
      /* ignore */
    }
  }
  try {
    return await invoke<T>(cmd, args);
  } finally {
    unlisten?.();
  }
}

export const runArchive = (
  days: number, dryRun: boolean, moveOriginals: boolean,
  onProgress?: (p: ScanProgress) => void,
) => runWithProgress<ArchiveSummary>(
  PROGRESS_EVENTS.archive, "archive",
  { days, dryRun, moveOriginals }, onProgress,
);

export const listArchived = () => invoke<ArchivedRow[]>("list_archived");

export const runRestore = (
  dryRun: boolean, purge: boolean,
  onProgress?: (p: ScanProgress) => void,
) => runWithProgress<RestoreSummary>(
  PROGRESS_EVENTS.restore, "restore",
  { dryRun, purge }, onProgress,
);

export const runCompact = (
  days: number, dryRun: boolean,
  onProgress?: (p: ScanProgress) => void,
) => runWithProgress<CompactSummary>(
  PROGRESS_EVENTS.compact, "compact",
  { days, dryRun }, onProgress,
);

export const runScanSensitive = (
  onProgress?: (p: ScanProgress) => void,
) => runWithProgress<SensitiveFile[]>(
  PROGRESS_EVENTS.scanSensitive, "scan_sensitive",
  {}, onProgress,
);

// ---- Phase 3: summarize / settings ----

export const runSummarize = (summaryOnly: boolean, ftsOnly: boolean) =>
  invoke<void>("summarize", { summaryOnly, ftsOnly });

export interface ConfigView {
  codex_sessions: string;
  hermes_sessions: string;
  default_archive_days: number;
  enabled_codex: boolean;
  enabled_hermes: boolean;
  language: string;
}

export const getConfig = () => invoke<ConfigView>("get_config");

export function setConfig(patch: {
  codex_sessions?: string;
  default_archive_days?: number;
  enabled_codex?: boolean;
  enabled_hermes?: boolean;
  language?: string;
}): Promise<void> {
  return invoke<void>("set_config", {
    codexSessions: patch.codex_sessions,
    defaultArchiveDays: patch.default_archive_days,
    enabledCodex: patch.enabled_codex,
    enabledHermes: patch.enabled_hermes,
    language: patch.language,
  });
}

export function fmtInt(n: number): string {
  return n.toLocaleString("en-US");
}
