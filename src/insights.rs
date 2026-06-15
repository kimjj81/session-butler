//! 인사이트: 색인된 SQLite 데이터 기반 사용 통계 리포트.
//!
//! `scan`으로 누적된 sessions/tool_usage 테이블을 집계해
//! 상위 tool/skill, 프로젝트, 월별 추세, 활동 시간대, 단어 빈도, 토큰 리더 등을 제공.

use crate::config::Config;
use crate::db::SessionDb;
use crate::error::{Error, Result};
use crate::i18n;
use crate::summary::STOP_WORDS;
use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;

const WEEKDAYS_EN: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const WEEKDAYS_KO: [&str; 7] = ["일", "월", "화", "수", "목", "금", "토"];

/// 인사이트 리포트 진입점.
pub fn run(config: Config, days: u64, top: usize, json: bool) -> Result<()> {
    let db = SessionDb::new(&config.codex_index_db)?;
    let limit = top as i64;

    if db.count_sessions()? == 0 {
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({"empty": true}))
                    .map_err(Error::Json)?
            );
        } else {
            println!("{}", i18n::insights_empty());
        }
        return Ok(());
    }

    // 집계 수집
    let (sessions, tokens, tool_calls, file_changes) = db.aggregate_totals(days)?;
    let archived = db.count_archived()?;
    let (date_from, date_to) = db.date_range(days)?;
    let distinct_projects = db.distinct_projects(days)?;
    let distinct_tools = db.distinct_tools(days)?;
    let top_tools = db.top_tools(days, limit)?;
    let least_tools = db.bottom_tools(days, limit)?;
    let projects = db.top_projects(days, limit)?;
    let months = db.count_by_month()?;
    let weekday = db.activity_by_weekday(days)?;
    let ids = db.session_ids_in_window(days)?;
    let prompts = db.first_user_prompts(days)?;
    let leaders = db.top_sessions_by_tokens(days, 5)?;

    let peak_hour = peak_hour_from_ids(&ids);
    let token_re = Regex::new(r"[A-Za-z0-9_가-힣./-]{2,}")
        .map_err(|e| Error::Other(format!("token regex: {}", e)))?;
    let top_words = top_words(&prompts, &token_re, top);

    let report = Report {
        window_days: days,
        overview: Overview {
            sessions,
            total_tokens: tokens,
            total_tool_calls: tool_calls,
            total_file_changes: file_changes,
            distinct_projects,
            distinct_tools,
            archived,
            date_from,
            date_to,
        },
        top_tools: map_tools(top_tools),
        least_used_tools: map_tools(least_tools),
        top_projects: projects.into_iter().map(|(r, s, t)| ProjectStat { repo: r, sessions: s, tokens: t }).collect(),
        monthly_trend: months.into_iter().map(|(m, s, tc, fc, t)| MonthStat {
            month: m, sessions: s, tool_calls: tc, file_changes: fc, tokens: t,
        }).collect(),
        activity_by_weekday: weekday.into_iter().map(|(wd, c)| WeekdayStat {
            weekday: weekday_name(wd as usize), weekday_index: wd as i64, sessions: c,
        }).collect(),
        peak_hour,
        top_words: top_words.into_iter().map(|(w, c)| WordStat { word: w, count: c as i64 }).collect(),
        token_leaders: leaders.into_iter().map(|(id, d, t, tc, p)| SessionStat {
            session_id: id, date: d, tokens: t, tool_calls: tc, prompt: p,
        }).collect(),
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&report).map_err(Error::Json)?);
        return Ok(());
    }

    render_text(&report);
    Ok(())
}

fn map_tools(v: Vec<(String, i64)>) -> Vec<ToolStat> {
    v.into_iter().map(|(t, c)| ToolStat { tool: t, calls: c }).collect()
}

// ---- 파생 계산 ----

/// session_id(예: 2026-02-07T15-53-52-uuid)에서 시작 시각(시)을 파싱해 피크 시간대 산출.
fn peak_hour_from_ids(ids: &[String]) -> Option<u32> {
    let mut buckets = [0u64; 24];
    for id in ids {
        if let Some(h) = parse_hour(id) {
            buckets[h as usize] += 1;
        }
    }
    let mut best = 0u32;
    let mut best_count = 0u64;
    for (h, &c) in buckets.iter().enumerate() {
        if c > best_count {
            best_count = c;
            best = h as u32;
        }
    }
    if best_count == 0 { None } else { Some(best) }
}

fn parse_hour(id: &str) -> Option<u32> {
    let after_t = id.split('T').nth(1)?;
    let hh = after_t.get(0..2)?;
    hh.parse::<u32>().ok().filter(|&h| h < 24)
}

/// first_user_prompt들을 토큰화해 상위 단어 추출 (불용어/숫자 제거).
fn top_words(prompts: &[String], token_re: &Regex, top: usize) -> Vec<(String, usize)> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    let trim_chars = ['.', '_', '-', '/'];
    for prompt in prompts {
        for m in token_re.find_iter(prompt) {
            let mut t = m.as_str();
            t = t.trim_start_matches(trim_chars);
            t = t.trim_end_matches(trim_chars);
            if t.is_empty() {
                continue;
            }
            let norm = if t.is_ascii() { t.to_ascii_lowercase() } else { t.to_string() };
            if STOP_WORDS.contains(&norm.as_str()) {
                continue;
            }
            if norm.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            *counts.entry(norm).or_insert(0) += 1;
        }
    }
    let mut v: Vec<_> = counts.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v.truncate(top);
    v
}

fn weekday_name(wd: usize) -> String {
    match i18n::lang() {
        i18n::Lang::Ko => WEEKDAYS_KO.get(wd).copied().unwrap_or("?").to_string(),
        i18n::Lang::En => WEEKDAYS_EN.get(wd).copied().unwrap_or("?").to_string(),
    }
}

fn fmt_tokens(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1e6)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1e3)
    } else {
        n.to_string()
    }
}

fn repo_short(url: &str) -> String {
    url.rsplit('/').next().unwrap_or(url).to_string()
}

// ---- 텍스트 렌더링 ----

fn render_text(r: &Report) {
    println!("\n{}", "=".repeat(60));
    println!("{}", i18n::insights_title());
    println!("{}", "-".repeat(60));
    println!("{}", i18n::insights_window(r.window_days));

    // Overview
    println!("\n■ {}", i18n::insights_section("overview"));
    let label = |k: &str| i18n::insights_section(k);
    println!("  {:<14} {}", label("sessions"), r.overview.sessions);
    println!("  {:<14} {}", label("tokens"), fmt_tokens(r.overview.total_tokens));
    println!("  {:<14} {}", label("tool_calls"), r.overview.total_tool_calls);
    println!("  {:<14} {}", label("file_changes"), r.overview.total_file_changes);
    println!("  {:<14} {}", label("projects"), r.overview.distinct_projects);
    println!("  {:<14} {}", label("tools_distinct"), r.overview.distinct_tools);
    println!("  {:<14} {}", label("archived"), r.overview.archived);
    let range = match (&r.overview.date_from, &r.overview.date_to) {
        (Some(a), Some(b)) => format!("{} ~ {}", a, b),
        _ => "-".to_string(),
    };
    println!("  {:<14} {}", label("date_range"), range);
    if let Some(h) = r.peak_hour {
        println!("  {:<14} {}:00", label("peak_hour"), h);
    }

    // Top tools
    println!("\n■ {} (top {})", i18n::insights_section("top_tools"), r.top_tools.len());
    print_tools(&r.top_tools);

    // Least-used tools
    println!("\n■ {}", i18n::insights_section("least_tools"));
    print_tools(&r.least_used_tools);

    // Top projects
    println!("\n■ {}", i18n::insights_section("projects"));
    if r.top_projects.is_empty() {
        println!("  {}", i18n::insights_empty());
    } else {
        println!("  {:<34} {:>7} {:>10}", label("repo"), label("sessions"), label("tokens"));
        for p in &r.top_projects {
            println!("  {:<34} {:>7} {:>10}", truncate(&repo_short(&p.repo), 34), p.sessions, fmt_tokens(p.tokens));
        }
    }

    // Monthly trend
    println!("\n■ {}", i18n::insights_section("trend"));
    if r.monthly_trend.is_empty() {
        println!("  {}", i18n::insights_empty());
    } else {
        println!("  {:<9} {:>7} {:>9} {:>9} {:>9}", label("month"), label("sessions"), label("tool_calls"), label("file_changes"), label("tokens"));
        for m in &r.monthly_trend {
            println!("  {:<9} {:>7} {:>9} {:>9} {:>9}", m.month, m.sessions, m.tool_calls, m.file_changes, fmt_tokens(m.tokens));
        }
    }

    // Activity by weekday
    println!("\n■ {}", i18n::insights_section("activity"));
    if r.activity_by_weekday.is_empty() {
        println!("  {}", i18n::insights_empty());
    } else {
        let max_c = r.activity_by_weekday.iter().map(|w| w.sessions).max().unwrap_or(1).max(1);
        for w in &r.activity_by_weekday {
            let bar_len = (w.sessions * 20 / max_c) as usize;
            let bar = "█".repeat(bar_len);
            println!("  {} {:>5}  {}", w.weekday, w.sessions, bar);
        }
    }

    // Top words
    println!("\n■ {}", i18n::insights_section("words"));
    if r.top_words.is_empty() {
        println!("  {}", i18n::insights_empty());
    } else {
        let mut line = String::from("  ");
        for (i, w) in r.top_words.iter().enumerate() {
            if i > 0 {
                line.push_str("   ");
            }
            line.push_str(&format!("{}({})", w.word, w.count));
        }
        println!("{}", line);
    }

    // Token leaders
    println!("\n■ {}", i18n::insights_section("leaders"));
    if r.token_leaders.is_empty() {
        println!("  {}", i18n::insights_empty());
    } else {
        for s in &r.token_leaders {
            let date = s.date.as_deref().unwrap_or("-");
            let prompt = s.prompt.as_deref().map(|p| truncate(p, 60)).unwrap_or_default();
            println!("  {} [{}] {} | {}",
                truncate(&s.session_id, 26), date, fmt_tokens(s.tokens), prompt);
            println!("       {}: {}", label("tool_calls"), s.tool_calls);
        }
    }

    println!("\n{}", "=".repeat(60));
}

fn print_tools(v: &[ToolStat]) {
    if v.is_empty() {
        println!("  {}", i18n::insights_empty());
        return;
    }
    let max_c = v.iter().map(|t| t.calls).max().unwrap_or(1).max(1);
    for t in v {
        let bar_len = (t.calls * 24 / max_c) as usize;
        let bar = "█".repeat(bar_len);
        println!("  {:<32} {:>7}  {}", truncate(&t.tool, 32), t.calls, bar);
    }
}

fn truncate(s: &str, limit: usize) -> String {
    if s.chars().count() <= limit {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(limit.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

// ---- 직렬화 구조체 ----

#[derive(Serialize)]
struct Report {
    window_days: u64,
    overview: Overview,
    top_tools: Vec<ToolStat>,
    least_used_tools: Vec<ToolStat>,
    top_projects: Vec<ProjectStat>,
    monthly_trend: Vec<MonthStat>,
    activity_by_weekday: Vec<WeekdayStat>,
    peak_hour: Option<u32>,
    top_words: Vec<WordStat>,
    token_leaders: Vec<SessionStat>,
}

#[derive(Serialize)]
struct Overview {
    sessions: i64,
    total_tokens: i64,
    total_tool_calls: i64,
    total_file_changes: i64,
    distinct_projects: i64,
    distinct_tools: i64,
    archived: i64,
    date_from: Option<String>,
    date_to: Option<String>,
}

#[derive(Serialize)]
struct ToolStat {
    tool: String,
    calls: i64,
}

#[derive(Serialize)]
struct ProjectStat {
    repo: String,
    sessions: i64,
    tokens: i64,
}

#[derive(Serialize)]
struct MonthStat {
    month: String,
    sessions: i64,
    tool_calls: i64,
    file_changes: i64,
    tokens: i64,
}

#[derive(Serialize)]
struct WeekdayStat {
    weekday: String,
    weekday_index: i64,
    sessions: i64,
}

#[derive(Serialize)]
struct WordStat {
    word: String,
    count: i64,
}

#[derive(Serialize)]
struct SessionStat {
    session_id: String,
    date: Option<String>,
    tokens: i64,
    tool_calls: i64,
    prompt: Option<String>,
}
