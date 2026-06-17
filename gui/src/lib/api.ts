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
  kind: "bar" | "spinner" | "inc" | "finish";
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

export function fmtInt(n: number): string {
  return n.toLocaleString("en-US");
}
