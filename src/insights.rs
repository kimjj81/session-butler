//! 인사이트: 색인된 SQLite 데이터 기반 사용 통계 리포트.
//!
//! `scan`으로 누적된 sessions/tool_usage 테이블을 집계해
//! 상위 tool/skill, 프로젝트, 월별 추세, 활동 시간대, 단어 빈도, 토큰 리더 등을 제공.

use crate::config::Config;
use crate::db::SessionDb;
use crate::error::{Error, Result};
use crate::i18n;
use crate::summary::STOP_WORDS;
use crate::util;
use chrono::Datelike;
use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;

const WEEKDAYS_EN: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const WEEKDAYS_KO: [&str; 7] = ["일", "월", "화", "수", "목", "금", "토"];

/// 시간 버킷 단위. clap ValueEnum 로 CLI `--by` 파싱.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, clap::ValueEnum)]
pub enum Granularity {
    #[value(name = "day")]
    Day,
    #[value(name = "week")]
    Week,
    #[value(name = "month")]
    Month,
}

impl Granularity {
    /// 버킷당 표시할 최빈 단어 수.
    fn words_per_bucket(self) -> usize {
        match self {
            Granularity::Day => 3,
            Granularity::Week => 4,
            Granularity::Month => 6,
        }
    }

    fn section_id(self) -> &'static str {
        match self {
            Granularity::Day => "trend_daily",
            Granularity::Week => "trend_weekly",
            Granularity::Month => "trend_monthly",
        }
    }
}

/// 인사이트 리포트 진입점.
pub fn run(config: Config, days: u64, top: usize, by: Granularity, json: bool) -> Result<()> {
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

    let token_re = Regex::new(r"[A-Za-z0-9_가-힣./-]{2,}")
        .map_err(|e| Error::Other(format!("token regex: {}", e)))?;

    // 집계 수집
    let (sessions, tokens, tool_calls, file_changes) = db.aggregate_totals(days)?;
    let archived = db.count_archived()?;
    let (date_from, date_to) = db.date_range(days)?;
    let distinct_projects = db.distinct_projects(days)?;
    let distinct_tools = db.distinct_tools(days)?;
    let top_tools = db.top_tools(days, limit)?;
    let least_tools = db.bottom_tools(days, limit)?;
    let projects = db.top_projects(days, limit)?;
    let weekday = db.activity_by_weekday(days)?;
    let ids = db.session_ids_in_window(days)?;
    let prompts = db.first_user_prompts(days)?;
    let leaders = db.top_sessions_by_tokens(days, 5)?;

    // 시간 버킷 집계 (세션/토큰/스킬/최빈단어)
    let detail = db.session_detail_window(days)?;
    let tool_rows = db.tool_usage_with_dates(days)?;
    let buckets = build_buckets(&detail, &tool_rows, by, &token_re);

    let peak_hour = peak_hour_from_ids(&ids);
    let top_words = top_words(&prompts, &token_re, top);

    let report = Report {
        window_days: days,
        granularity: by,
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
        time_buckets: buckets,
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

/// 프롬프트에서 정규화된 유효 토큰(불용어/숫자 제거) 추출.
fn valid_tokens(prompt: &str, token_re: &Regex) -> Vec<String> {
    let trim_chars = ['.', '_', '-', '/'];
    let mut out = Vec::new();
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
        out.push(norm);
    }
    out
}

/// first_user_prompt들을 토큰화해 상위 단어 추출 (불용어/숫자 제거).
fn top_words(prompts: &[String], token_re: &Regex, top: usize) -> Vec<(String, usize)> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for prompt in prompts {
        for w in valid_tokens(prompt, token_re) {
            *counts.entry(w).or_insert(0) += 1;
        }
    }
    let mut v: Vec<_> = counts.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v.truncate(top);
    v
}

/// 날짜 문자열(yyyy-mm-dd)을 버킷 라벨로 변환. 파싱 실패 시 None.
fn bucket_label(date_str: &str, g: Granularity) -> Option<String> {
    let d = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()?;
    Some(match g {
        Granularity::Day => d.to_string(),
        Granularity::Week => {
            let iw = d.iso_week();
            format!("{}-W{:02}", iw.year(), iw.week())
        }
        Granularity::Month => format!("{:04}-{:02}", d.year(), d.month()),
    })
}

#[derive(Default)]
struct BucketAcc {
    sessions: i64,
    tokens: i64,
    words: HashMap<String, i64>,
    tools: HashMap<String, i64>,
}

/// 세션 상세 + tool 사용(날짜 포함)을 버킷별로 집계.
fn build_buckets(
    detail: &[(Option<String>, String, i64, Option<String>)],
    tool_rows: &[(Option<String>, String, i64)],
    by: Granularity,
    token_re: &Regex,
) -> Vec<TimeBucket> {
    let mut accs: HashMap<String, BucketAcc> = HashMap::new();

    for (date, _sid, tokens, prompt) in detail {
        let Some(date_str) = date else { continue };
        let Some(label) = bucket_label(date_str, by) else { continue };
        let a = accs.entry(label).or_default();
        a.sessions += 1;
        a.tokens += tokens;
        if let Some(p) = prompt {
            for w in valid_tokens(p, token_re) {
                *a.words.entry(w).or_insert(0) += 1;
            }
        }
    }

    for (date, tool, count) in tool_rows {
        let Some(date_str) = date else { continue };
        let Some(label) = bucket_label(date_str, by) else { continue };
        if let Some(a) = accs.get_mut(&label) {
            *a.tools.entry(tool.clone()).or_insert(0) += count;
        }
    }

    let words_n = by.words_per_bucket();
    let mut buckets: Vec<TimeBucket> = accs
        .into_iter()
        .map(|(label, a)| {
            let (top_skill, top_skill_calls) = a
                .tools
                .into_iter()
                .max_by(|x, y| x.1.cmp(&y.1))
                .unwrap_or_default();
            let mut words: Vec<_> = a.words.into_iter().collect();
            words.sort_by(|x, y| y.1.cmp(&x.1).then(x.0.cmp(&y.0)));
            TimeBucket {
                label,
                sessions: a.sessions,
                tokens: a.tokens,
                top_skill: if top_skill.is_empty() { None } else { Some(top_skill) },
                top_skill_calls,
                top_words: words.into_iter().take(words_n).map(|(w, _)| w).collect(),
            }
        })
        .collect();
    // 최근이 위에 오도록 내림차순
    buckets.sort_by(|a, b| b.label.cmp(&a.label));
    buckets
}

fn weekday_name(wd: usize) -> String {
    match i18n::lang() {
        i18n::Lang::Ko => WEEKDAYS_KO.get(wd).copied().unwrap_or("?").to_string(),
        i18n::Lang::En => WEEKDAYS_EN.get(wd).copied().unwrap_or("?").to_string(),
    }
}

/// 토큰 수 표시 — 쉼표 구분 (예: 16,236,291,196).
fn fmt_tokens(n: i64) -> String {
    util::fmt_int(n)
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
    println!("  {:<14} {}", label("sessions"), util::fmt_int(r.overview.sessions));
    println!("  {:<14} {}", label("tokens"), fmt_tokens(r.overview.total_tokens));
    println!("  {:<14} {}", label("tool_calls"), util::fmt_int(r.overview.total_tool_calls));
    println!("  {:<14} {}", label("file_changes"), util::fmt_int(r.overview.total_file_changes));
    println!("  {:<14} {}", label("projects"), util::fmt_int(r.overview.distinct_projects));
    println!("  {:<14} {}", label("tools_distinct"), util::fmt_int(r.overview.distinct_tools));
    println!("  {:<14} {}", label("archived"), util::fmt_int(r.overview.archived));
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
        println!("  {:<34} {:>9} {:>16}", label("repo"), label("sessions"), label("tokens"));
        for p in &r.top_projects {
            println!("  {:<34} {:>9} {:>16}", truncate(&repo_short(&p.repo), 34), util::fmt_int(p.sessions), fmt_tokens(p.tokens));
        }
    }

    // Time-bucketed trend (day/week/month): sessions / tokens / top skill / top words
    println!("\n■ {}", i18n::insights_section(r.granularity.section_id()));
    if r.time_buckets.is_empty() {
        println!("  {}", i18n::insights_empty());
    } else {
        println!(
            "  {:<12} {:>7} {:>16}  {:<22} {}",
            label("bucket"), label("sessions"), label("tokens"), label("top_skill"), label("top_words")
        );
        for b in &r.time_buckets {
            let skill = match &b.top_skill {
                Some(s) => truncate(s, 22),
                None => "-".to_string(),
            };
            let words = if b.top_words.is_empty() { "-".to_string() } else { b.top_words.join(", ") };
            println!(
                "  {:<12} {:>7} {:>16}  {:<22} {}",
                b.label, util::fmt_int(b.sessions), fmt_tokens(b.tokens), skill, words
            );
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
            println!("  {} {:>7}  {}", w.weekday, util::fmt_int(w.sessions), bar);
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
            line.push_str(&format!("{}({})", w.word, util::fmt_int(w.count)));
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
            println!("       {}: {}", label("tool_calls"), util::fmt_int(s.tool_calls));
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
        println!("  {:<32} {:>12}  {}", truncate(&t.tool, 32), util::fmt_int(t.calls), bar);
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
    granularity: Granularity,
    overview: Overview,
    top_tools: Vec<ToolStat>,
    least_used_tools: Vec<ToolStat>,
    top_projects: Vec<ProjectStat>,
    time_buckets: Vec<TimeBucket>,
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
struct TimeBucket {
    label: String,
    sessions: i64,
    tokens: i64,
    top_skill: Option<String>,
    top_skill_calls: i64,
    top_words: Vec<String>,
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
