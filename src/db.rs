//! SQLite 데이터베이스 연산 (FTS5 포함)

use crate::error::{Error, Result};
use crate::types::{ArchivedSessionRow, CodexSessionMeta, SessionInfo};
use chrono::Utc;
use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 세션 인덱스 데이터베이스 매니저
pub struct SessionDb {
    conn: Connection,
}

impl SessionDb {
    /// 새 데이터베이스 연결 생성
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::Io(e))?;
        }

        let conn = Connection::open(path)
            .map_err(|e| Error::Sqlite(e))?;

        let db = Self { conn };
        db.init_tables()?;
        Ok(db)
    }

    /// 테이블 초기화
    fn init_tables(&self) -> Result<()> {
        // crash-safety + 성능 (단일 연결이라 동시성 이점은 없지만 내구성 향상)
        self.conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .map_err(|e| Error::Sqlite(e))?;

        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS sessions (
                session_id TEXT PRIMARY KEY,
                path TEXT NOT NULL,
                date TEXT,
                cwd TEXT,
                first_user_prompt TEXT,
                model_provider TEXT,
                cli_version TEXT,
                source TEXT,
                model TEXT,
                git_sha TEXT,
                git_branch TEXT,
                git_origin_url TEXT,
                tool_call_count INTEGER DEFAULT 0,
                file_change_count INTEGER DEFAULT 0,
                total_tokens INTEGER DEFAULT 0,
                line_count INTEGER DEFAULT 0,
                corrupt_lines INTEGER DEFAULT 0,
                has_user_event INTEGER DEFAULT 0,
                size_bytes INTEGER DEFAULT 0,
                indexed_at TEXT,
                archived INTEGER DEFAULT 0,
                compressed_path TEXT,
                checksum_sha256 TEXT,
                archived_at TEXT
            )",
            [],
        ).map_err(|e| Error::Sqlite(e))?;

        // FTS5 가상 테이블
        self.conn.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS sessions_fts
             USING fts5(
                session_id,
                first_user_prompt,
                cwd,
                git_origin_url
             )",
            [],
        ).map_err(|e| Error::Sqlite(e))?;

        // tool/skill별 호출 수 (insights 집계용)
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS tool_usage (
                session_id TEXT NOT NULL,
                tool_name  TEXT NOT NULL,
                call_count INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (session_id, tool_name)
            )",
            [],
        ).map_err(|e| Error::Sqlite(e))?;

        // 대화 본문 단어 빈도 — 카테고리(conversation/reasoning/tools)별 (insights --words용)
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS session_words (
                session_id TEXT NOT NULL,
                category   TEXT NOT NULL,
                word       TEXT NOT NULL,
                count      INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (session_id, category, word)
            )",
            [],
        ).map_err(|e| Error::Sqlite(e))?;

        self.migrate()?;

        Ok(())
    }

    /// 스키마 마이그레이션 (user_version 기반)
    fn migrate(&self) -> Result<()> {
        let version: i64 = self.conn.query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|e| Error::Sqlite(e))?;

        if version < 1 {
            let existing = self.existing_columns("sessions")?;
            // 기존 DB 호환: 누락된 컬럼만 추가
            let additions: &[(&str, &str)] = &[
                ("size_bytes", "INTEGER DEFAULT 0"),
                ("archived", "INTEGER DEFAULT 0"),
                ("compressed_path", "TEXT"),
                ("checksum_sha256", "TEXT"),
                ("archived_at", "TEXT"),
            ];
            for (col, def) in additions {
                if !existing.iter().any(|c| c == col) {
                    let sql = format!("ALTER TABLE sessions ADD COLUMN {} {}", col, def);
                    self.conn.execute(&sql, [])
                        .map_err(|e| Error::Sqlite(e))?;
                }
            }
            self.conn.execute("PRAGMA user_version = 1", [])
                .map_err(|e| Error::Sqlite(e))?;
        }

        if version < 2 {
            // tool_usage 테이블 (init_tables에서도 생성하지만 구버전 DB 호환용)
            self.conn.execute(
                "CREATE TABLE IF NOT EXISTS tool_usage (
                    session_id TEXT NOT NULL,
                    tool_name  TEXT NOT NULL,
                    call_count INTEGER NOT NULL DEFAULT 0,
                    PRIMARY KEY (session_id, tool_name)
                )",
                [],
            ).map_err(|e| Error::Sqlite(e))?;
            self.conn.execute("PRAGMA user_version = 2", [])
                .map_err(|e| Error::Sqlite(e))?;
        }

        if version < 3 {
            // session_words 테이블 (대화 본문 단어 빈도). init_tables에서도 생성하지만 구버전 DB 호환용.
            self.conn.execute(
                "CREATE TABLE IF NOT EXISTS session_words (
                    session_id TEXT NOT NULL,
                    word       TEXT NOT NULL,
                    count      INTEGER NOT NULL DEFAULT 0,
                    PRIMARY KEY (session_id, word)
                )",
                [],
            ).map_err(|e| Error::Sqlite(e))?;
            self.conn.execute("PRAGMA user_version = 3", [])
                .map_err(|e| Error::Sqlite(e))?;
        }

        if version < 4 {
            // session_words 에 category 컬럼 추가 — 기존 스키마(category 없음)를 버리고
            // 새 스키마로 재생성. session_words 는 v3에서 막 도입된 테이블이고 재스캔으로
            // 백필되므로 drop해도 안전하다.
            self.conn.execute("DROP TABLE IF EXISTS session_words", [])
                .map_err(|e| Error::Sqlite(e))?;
            self.conn.execute(
                "CREATE TABLE session_words (
                    session_id TEXT NOT NULL,
                    category   TEXT NOT NULL,
                    word       TEXT NOT NULL,
                    count      INTEGER NOT NULL DEFAULT 0,
                    PRIMARY KEY (session_id, category, word)
                )",
                [],
            ).map_err(|e| Error::Sqlite(e))?;
            self.conn.execute("PRAGMA user_version = 4", [])
                .map_err(|e| Error::Sqlite(e))?;
        }

        Ok(())
    }

    /// 테이블의 컬럼 이름 목록 조회
    fn existing_columns(&self, table: &str) -> Result<Vec<String>> {
        let sql = format!("PRAGMA table_info({})", table);
        let mut stmt = self.conn.prepare(&sql)
            .map_err(|e| Error::Sqlite(e))?;
        let rows = stmt.query_map([], |row| {
            let name: String = row.get(1)?;
            Ok(name)
        }).map_err(|e| Error::Sqlite(e))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| Error::Sqlite(e))?;
        Ok(rows)
    }

    /// 세션 upsert
    pub fn upsert_session(&self, meta: &CodexSessionMeta) -> Result<()> {
        let indexed_at = meta.indexed_at
            .unwrap_or_else(|| Utc::now())
            .to_rfc3339();

        let has_user_event = if meta.has_user_event { 1 } else { 0 };

        let mut stmt = self.conn.prepare_cached(
            "INSERT INTO sessions (session_id, path, date, cwd, first_user_prompt,
                                  model_provider, cli_version, source, model,
                                  git_sha, git_branch, git_origin_url,
                                  tool_call_count, file_change_count, total_tokens,
                                  line_count, corrupt_lines, has_user_event, size_bytes, indexed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)
             ON CONFLICT(session_id) DO UPDATE SET
                path=excluded.path, date=excluded.date, cwd=excluded.cwd,
                first_user_prompt=excluded.first_user_prompt, model_provider=excluded.model_provider,
                cli_version=excluded.cli_version, source=excluded.source, model=excluded.model,
                git_sha=excluded.git_sha, git_branch=excluded.git_branch,
                git_origin_url=excluded.git_origin_url, tool_call_count=excluded.tool_call_count,
                file_change_count=excluded.file_change_count, total_tokens=excluded.total_tokens,
                line_count=excluded.line_count, corrupt_lines=excluded.corrupt_lines,
                has_user_event=excluded.has_user_event, size_bytes=excluded.size_bytes, indexed_at=excluded.indexed_at"
        ).map_err(|e| Error::Sqlite(e))?;

        stmt.execute(params![
            &meta.session_id,
            meta.path.to_string_lossy().as_ref(),
            &meta.date,
            &meta.cwd,
            &meta.first_user_prompt,
            &meta.model_provider,
            &meta.cli_version,
            &meta.source,
            &meta.model,
            &meta.git_sha,
            &meta.git_branch,
            &meta.git_origin_url,
            meta.tool_call_count as i64,
            meta.file_change_count as i64,
            meta.total_tokens as i64,
            meta.line_count as i64,
            meta.corrupt_lines as i64,
            has_user_event,
            meta.size_bytes as i64,
            &indexed_at,
        ]).map_err(|e| Error::Sqlite(e))?;

        // FTS5 업데이트 (기존 행 삭제 후 삽입 - 재인덱싱 시 중복 적재 방지)
        let first_prompt = meta.first_user_prompt.as_deref().unwrap_or("");
        let cwd = meta.cwd.as_deref().unwrap_or("");
        let git_url = meta.git_origin_url.as_deref().unwrap_or("");

        self.conn.execute(
            "DELETE FROM sessions_fts WHERE session_id = ?1",
            params![&meta.session_id],
        ).map_err(|e| Error::Sqlite(e))?;

        let mut fts_stmt = self.conn.prepare_cached(
            "INSERT INTO sessions_fts(session_id, first_user_prompt, cwd, git_origin_url)
             VALUES (?1, ?2, ?3, ?4)"
        ).map_err(|e| Error::Sqlite(e))?;

        fts_stmt.execute(params![
            &meta.session_id,
            first_prompt,
            cwd,
            git_url,
        ]).map_err(|e| Error::Sqlite(e))?;

        // tool_usage 갱신 (기존 행 삭제 후 재삽입 — 재인덱싱 시 중복 방지)
        self.upsert_tool_usage(&meta.session_id, &meta.tool_usage)?;

        // session_words 갱신 (기존 행 삭제 후 재삽입 — 재인덱싱 시 중복 방지)
        self.upsert_session_words(&meta.session_id, &meta.word_counts)?;

        Ok(())
    }

    /// tool_usage 행 갱신: 기존 행 삭제 후 현재 분포 삽입.
    /// 빈 map이면 삭제만 수행(이전에 기록된 tool 사용이 사라진 경우 정합성 유지).
    fn upsert_tool_usage(&self, session_id: &str, usage: &HashMap<String, usize>) -> Result<()> {
        self.conn.execute(
            "DELETE FROM tool_usage WHERE session_id = ?1",
            params![session_id],
        ).map_err(|e| Error::Sqlite(e))?;

        if usage.is_empty() {
            return Ok(());
        }

        let mut stmt = self.conn.prepare_cached(
            "INSERT INTO tool_usage (session_id, tool_name, call_count) VALUES (?1, ?2, ?3)"
        ).map_err(|e| Error::Sqlite(e))?;

        for (tool_name, count) in usage {
            stmt.execute(params![session_id, tool_name, *count as i64])
                .map_err(|e| Error::Sqlite(e))?;
        }

        Ok(())
    }

    /// session_words 행 갱신: 기존 행 삭제 후 현재 분포 삽입.
    /// words는 category → word → count. 빈 값이면 삭제만 수행(정합성 유지).
    fn upsert_session_words(&self, session_id: &str, words: &HashMap<String, HashMap<String, usize>>) -> Result<()> {
        self.conn.execute(
            "DELETE FROM session_words WHERE session_id = ?1",
            params![session_id],
        ).map_err(|e| Error::Sqlite(e))?;

        let mut stmt = self.conn.prepare_cached(
            "INSERT INTO session_words (session_id, category, word, count) VALUES (?1, ?2, ?3, ?4)"
        ).map_err(|e| Error::Sqlite(e))?;

        for (category, counts) in words {
            for (word, count) in counts {
                stmt.execute(params![session_id, category, word, *count as i64])
                    .map_err(|e| Error::Sqlite(e))?;
            }
        }

        Ok(())
    }

    /// 세션 수 조회
    pub fn count_sessions(&self) -> Result<i64> {
        let mut stmt = self.conn.prepare("SELECT COUNT(*) FROM sessions")
            .map_err(|e| Error::Sqlite(e))?;

        let count = stmt.query_row([], |row| row.get(0))
            .map_err(|e| Error::Sqlite(e))?;

        Ok(count)
    }

    /// DB 행 → ArchivedSessionRow 변환 (associated helper)
    fn archived_row(row: &rusqlite::Row) -> rusqlite::Result<ArchivedSessionRow> {
        Ok(ArchivedSessionRow {
            session_id: row.get(0)?,
            path: PathBuf::from(row.get::<_, Option<String>>(1)?.unwrap_or_default()),
            date: row.get(2)?,
            compressed_path: PathBuf::from(row.get::<_, Option<String>>(3)?.unwrap_or_default()),
            checksum_sha256: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
        })
    }

    /// days일 이후 cutoff 날짜 문자열 (ISO yyyy-mm-dd)
    fn days_cutoff(days: u64) -> String {
        (Utc::now() - chrono::Duration::days(days as i64)).date_naive().to_string()
    }

    /// 세션을 archived로 표시 (보관본 경로/체크섬 기록). 영향 행 수==1 검증.
    pub fn mark_archived(&self, session_id: &str, compressed_path: &Path, checksum: &str) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE sessions SET archived = 1, compressed_path = ?1, checksum_sha256 = ?2, archived_at = ?3
             WHERE session_id = ?4",
            params![
                compressed_path.to_string_lossy().as_ref(),
                checksum,
                Utc::now().to_rfc3339(),
                session_id,
            ],
        ).map_err(|e| Error::Sqlite(e))?;
        if n == 0 {
            return Err(Error::Other(format!("mark_archived: session_id not found: {}", session_id)));
        }
        Ok(())
    }

    /// archived 해제 + 보관본 메타 제거 (restore --purge). 영향 행 수==1 검증.
    pub fn mark_purged(&self, session_id: &str) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE sessions SET archived = 0, compressed_path = NULL, checksum_sha256 = NULL, archived_at = NULL
             WHERE session_id = ?1",
            params![session_id],
        ).map_err(|e| Error::Sqlite(e))?;
        if n == 0 {
            return Err(Error::Other(format!("mark_purged: session_id not found: {}", session_id)));
        }
        Ok(())
    }

    /// archived 세션 전체 조회
    pub fn list_archived(&self) -> Result<Vec<ArchivedSessionRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT session_id, path, date, compressed_path, checksum_sha256
             FROM sessions WHERE archived = 1"
        ).map_err(|e| Error::Sqlite(e))?;
        let rows = stmt.query_map([], Self::archived_row)
            .map_err(|e| Error::Sqlite(e))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Sqlite(e))?;
        Ok(rows)
    }

    /// session_id 목록으로 archived 세션 조회 (restore --session-id)
    pub fn list_archived_by_ids(&self, ids: &[String]) -> Result<Vec<ArchivedSessionRow>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let placeholders = (0..ids.len()).map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT session_id, path, date, compressed_path, checksum_sha256
             FROM sessions WHERE archived = 1 AND session_id IN ({})",
            placeholders
        );
        let mut stmt = self.conn.prepare(&sql).map_err(|e| Error::Sqlite(e))?;
        let rows = stmt.query_map(rusqlite::params_from_iter(ids.iter()), Self::archived_row)
            .map_err(|e| Error::Sqlite(e))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Sqlite(e))?;
        Ok(rows)
    }

    /// 최근 days일 이내의 archived 세션 조회 (restore -d N)
    pub fn list_archived_by_days(&self, days: u64) -> Result<Vec<ArchivedSessionRow>> {
        let cutoff = Self::days_cutoff(days);
        let mut stmt = self.conn.prepare(
            "SELECT session_id, path, date, compressed_path, checksum_sha256
             FROM sessions WHERE archived = 1 AND date >= ?1"
        ).map_err(|e| Error::Sqlite(e))?;
        let rows = stmt.query_map(params![cutoff], Self::archived_row)
            .map_err(|e| Error::Sqlite(e))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Sqlite(e))?;
        Ok(rows)
    }

    /// 보존 기간(days)보다 오래된 활성(archived 아님) 세션 조회 (archive 대상 선정).
    /// 최근 days일은 보존하고 그 이전 세션을 압축 대상으로 반환한다 (compact와 동일 방향).
    pub fn list_archive_candidates(&self, days: u64) -> Result<Vec<SessionInfo>> {
        let cutoff = Self::days_cutoff(days);
        let mut stmt = self.conn.prepare(
            "SELECT path, date, size_bytes, session_id, model_provider, cli_version
             FROM sessions WHERE archived = 0 AND date < ?1"
        ).map_err(|e| Error::Sqlite(e))?;
        let rows = stmt.query_map(params![cutoff], |row| {
            let path: String = row.get::<_, Option<String>>(0)?.unwrap_or_default();
            let date_str: Option<String> = row.get(1)?;
            let size_bytes: i64 = row.get(2).unwrap_or(0);
            let session_id: Option<String> = row.get(3)?;
            let model_provider: Option<String> = row.get(4)?;
            let cli_version: Option<String> = row.get(5)?;
            let date = date_str.as_deref()
                .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
            Ok(SessionInfo {
                path: PathBuf::from(path),
                date,
                size_bytes: size_bytes as u64,
                session_id,
                model_provider,
                cli_version,
                archived: false,
            })
        }).map_err(|e| Error::Sqlite(e))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| Error::Sqlite(e))?;
        Ok(rows)
    }

    /// 최근 days일 이내의 전체 세션(archived 포함) 조회 (list/stats 표시용)
    pub fn list_sessions_for_display(&self, days: u64) -> Result<Vec<SessionInfo>> {
        let cutoff = Self::days_cutoff(days);
        let mut stmt = self.conn.prepare(
            "SELECT path, date, size_bytes, session_id, model_provider, cli_version, archived
             FROM sessions WHERE date >= ?1"
        ).map_err(|e| Error::Sqlite(e))?;
        let rows = stmt.query_map(params![cutoff], |row| {
            let path: String = row.get::<_, Option<String>>(0)?.unwrap_or_default();
            let date_str: Option<String> = row.get(1)?;
            let size_bytes: i64 = row.get(2).unwrap_or(0);
            let session_id: Option<String> = row.get(3)?;
            let model_provider: Option<String> = row.get(4)?;
            let cli_version: Option<String> = row.get(5)?;
            let archived: i64 = row.get(6).unwrap_or(0);
            let date = date_str.as_deref()
                .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
            Ok(SessionInfo {
                path: PathBuf::from(path),
                date,
                size_bytes: size_bytes as u64,
                session_id,
                model_provider,
                cli_version,
                archived: archived != 0,
            })
        }).map_err(|e| Error::Sqlite(e))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| Error::Sqlite(e))?;
        Ok(rows)
    }

    /// archived 세션 수 (stats용)
    pub fn count_archived(&self) -> Result<i64> {
        let mut stmt = self.conn.prepare("SELECT COUNT(*) FROM sessions WHERE archived = 1")
            .map_err(|e| Error::Sqlite(e))?;
        let count = stmt.query_row([], |row| row.get(0))
            .map_err(|e| Error::Sqlite(e))?;
        Ok(count)
    }

    /// FTS5 검색
    pub fn search_fts(&self, query: &str, limit: i64) -> Result<Vec<(String, Option<String>, Option<String>, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.session_id, s.date, s.cwd, s.first_user_prompt
             FROM sessions s
             WHERE s.session_id IN (
                 SELECT session_id FROM sessions_fts WHERE sessions_fts MATCH ?1 LIMIT ?2
             )"
        ).map_err(|e| Error::Sqlite(e))?;

        let rows = stmt.query_map(params![query, limit], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
            ))
        }).map_err(|e| Error::Sqlite(e))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| Error::Sqlite(e))?;

        Ok(rows)
    }

    /// 월별 세션 수 조회
    pub fn count_by_month(&self) -> Result<Vec<(String, i64, i64, i64, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT substr(date, 1, 7) AS month, COUNT(*) as cnt,
                    SUM(tool_call_count) as total_tools,
                    SUM(file_change_count) as total_changes,
                    SUM(total_tokens) as total_tokens
             FROM sessions WHERE date IS NOT NULL
             GROUP BY month ORDER BY month"
        ).map_err(|e| Error::Sqlite(e))?;

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        }).map_err(|e| Error::Sqlite(e))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| Error::Sqlite(e))?;

        Ok(rows)
    }

    /// 윈도우 하한(yyyy-mm-dd). days=0 → 전체 기간을 뜻하는 sentinel '0000-00-00'.
    /// (모든 실제 날짜 문자열이 이보다 사전순 크므로 전체 포함)
    fn cutoff_bound(days: u64) -> String {
        if days == 0 {
            "0000-00-00".to_string()
        } else {
            Self::days_cutoff(days)
        }
    }

    /// 개요 집계: (세션수, 총토큰, 총툴콜, 총파일변경)
    pub fn aggregate_totals(&self, days: u64) -> Result<(i64, i64, i64, i64)> {
        let cutoff = Self::cutoff_bound(days);
        let mut stmt = self.conn.prepare(
            "SELECT COUNT(*), COALESCE(SUM(total_tokens),0), \
             COALESCE(SUM(tool_call_count),0), COALESCE(SUM(file_change_count),0) \
             FROM sessions WHERE date >= ?1"
        ).map_err(|e| Error::Sqlite(e))?;
        let row = stmt.query_row(params![cutoff], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        }).map_err(|e| Error::Sqlite(e))?;
        Ok(row)
    }

    /// 날짜 범위 (min, max)
    pub fn date_range(&self, days: u64) -> Result<(Option<String>, Option<String>)> {
        let cutoff = Self::cutoff_bound(days);
        let mut stmt = self.conn.prepare(
            "SELECT MIN(date), MAX(date) FROM sessions WHERE date IS NOT NULL AND date >= ?1"
        ).map_err(|e| Error::Sqlite(e))?;
        let r = stmt.query_row(params![cutoff], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| Error::Sqlite(e))?;
        Ok(r)
    }

    /// 고유 프로젝트(git_origin_url) 수
    pub fn distinct_projects(&self, days: u64) -> Result<i64> {
        let cutoff = Self::cutoff_bound(days);
        let mut stmt = self.conn.prepare(
            "SELECT COUNT(DISTINCT git_origin_url) FROM sessions \
             WHERE git_origin_url IS NOT NULL AND git_origin_url <> '' AND date >= ?1"
        ).map_err(|e| Error::Sqlite(e))?;
        let n = stmt.query_row(params![cutoff], |r| r.get(0))
            .map_err(|e| Error::Sqlite(e))?;
        Ok(n)
    }

    /// 고유 tool/skill 수
    pub fn distinct_tools(&self, days: u64) -> Result<i64> {
        let cutoff = Self::cutoff_bound(days);
        let mut stmt = self.conn.prepare(
            "SELECT COUNT(DISTINCT u.tool_name) FROM tool_usage u \
             JOIN sessions s ON s.session_id = u.session_id WHERE s.date >= ?1"
        ).map_err(|e| Error::Sqlite(e))?;
        let n = stmt.query_row(params![cutoff], |r| r.get(0))
            .map_err(|e| Error::Sqlite(e))?;
        Ok(n)
    }

    /// tool/skill 순위 (name, 총 호출수). ascending=false: 많이 쓴 순.
    fn tools_ranked(&self, days: u64, limit: i64, ascending: bool) -> Result<Vec<(String, i64)>> {
        let dir = if ascending { "ASC" } else { "DESC" };
        let cutoff = Self::cutoff_bound(days);
        let sql = format!(
            "SELECT u.tool_name, SUM(u.call_count) AS n \
             FROM tool_usage u JOIN sessions s ON s.session_id = u.session_id \
             WHERE s.date >= ?1 AND u.call_count > 0 \
             GROUP BY u.tool_name ORDER BY n {dir}, u.tool_name LIMIT ?2"
        );
        let mut stmt = self.conn.prepare(&sql).map_err(|e| Error::Sqlite(e))?;
        let rows = stmt.query_map(params![cutoff, limit], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| Error::Sqlite(e))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| Error::Sqlite(e))?;
        Ok(rows)
    }

    /// 많이 쓴 tool/skill 순
    pub fn top_tools(&self, days: u64, limit: i64) -> Result<Vec<(String, i64)>> {
        self.tools_ranked(days, limit, false)
    }

    /// 적게 쓴 tool/skill 순
    pub fn bottom_tools(&self, days: u64, limit: i64) -> Result<Vec<(String, i64)>> {
        self.tools_ranked(days, limit, true)
    }

    /// 상위 프로젝트 (git_origin_url, 세션수, 토큰합)
    pub fn top_projects(&self, days: u64, limit: i64) -> Result<Vec<(String, i64, i64)>> {
        let cutoff = Self::cutoff_bound(days);
        let mut stmt = self.conn.prepare(
            "SELECT git_origin_url, COUNT(*) AS c, COALESCE(SUM(total_tokens),0) \
             FROM sessions WHERE git_origin_url IS NOT NULL AND git_origin_url <> '' AND date >= ?1 \
             GROUP BY git_origin_url ORDER BY c DESC, git_origin_url LIMIT ?2"
        ).map_err(|e| Error::Sqlite(e))?;
        let rows = stmt.query_map(params![cutoff, limit], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(|e| Error::Sqlite(e))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| Error::Sqlite(e))?;
        Ok(rows)
    }

    /// 토큰 상위 세션 (session_id, date, tokens, tool_calls, first_user_prompt)
    pub fn top_sessions_by_tokens(
        &self,
        days: u64,
        limit: i64,
    ) -> Result<Vec<(String, Option<String>, i64, i64, Option<String>)>> {
        let cutoff = Self::cutoff_bound(days);
        let mut stmt = self.conn.prepare(
            "SELECT session_id, date, total_tokens, tool_call_count, first_user_prompt \
             FROM sessions WHERE total_tokens > 0 AND date >= ?1 \
             ORDER BY total_tokens DESC LIMIT ?2"
        ).map_err(|e| Error::Sqlite(e))?;
        let rows = stmt.query_map(params![cutoff, limit], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        }).map_err(|e| Error::Sqlite(e))?
          .collect::<rusqlite::Result<Vec<_>>>()
          .map_err(|e| Error::Sqlite(e))?;
        Ok(rows)
    }

    /// 요일별 세션 수 (0=일요일..6=토요일)
    pub fn activity_by_weekday(&self, days: u64) -> Result<Vec<(i64, i64)>> {
        let cutoff = Self::cutoff_bound(days);
        let mut stmt = self.conn.prepare(
            "SELECT CAST(strftime('%w', date) AS INTEGER) AS wd, COUNT(*) \
             FROM sessions WHERE date IS NOT NULL AND date >= ?1 GROUP BY wd ORDER BY wd"
        ).map_err(|e| Error::Sqlite(e))?;
        let rows = stmt.query_map(params![cutoff], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| Error::Sqlite(e))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| Error::Sqlite(e))?;
        Ok(rows)
    }

    /// 윈도우 내 session_id 목록 (피크 시각 분석용)
    pub fn session_ids_in_window(&self, days: u64) -> Result<Vec<String>> {
        let cutoff = Self::cutoff_bound(days);
        let mut stmt = self.conn.prepare(
            "SELECT session_id FROM sessions WHERE session_id IS NOT NULL AND date >= ?1"
        ).map_err(|e| Error::Sqlite(e))?;
        let rows = stmt.query_map(params![cutoff], |r| r.get::<_, String>(0))
            .map_err(|e| Error::Sqlite(e))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| Error::Sqlite(e))?;
        Ok(rows)
    }

    /// first_user_prompt 목록 (단어 빈도 분석용)
    pub fn first_user_prompts(&self, days: u64) -> Result<Vec<String>> {
        let cutoff = Self::cutoff_bound(days);
        let mut stmt = self.conn.prepare(
            "SELECT first_user_prompt FROM sessions \
             WHERE first_user_prompt IS NOT NULL AND first_user_prompt <> '' AND date >= ?1"
        ).map_err(|e| Error::Sqlite(e))?;
        let rows = stmt.query_map(params![cutoff], |r| r.get::<_, String>(0))
            .map_err(|e| Error::Sqlite(e))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| Error::Sqlite(e))?;
        Ok(rows)
    }

    /// 특정 카테고리 단어 순위 (word, 총 빈도). 많이 쓴 순.
    pub fn top_words_category(&self, days: u64, category: &str, limit: i64) -> Result<Vec<(String, i64)>> {
        let cutoff = Self::cutoff_bound(days);
        let mut stmt = self.conn.prepare(
            "SELECT w.word, SUM(w.count) AS n \
             FROM session_words w JOIN sessions s ON s.session_id = w.session_id \
             WHERE s.date >= ?1 AND w.category = ?2 AND w.count > 0 \
             GROUP BY w.word ORDER BY n DESC, w.word LIMIT ?3"
        ).map_err(|e| Error::Sqlite(e))?;
        let rows = stmt.query_map(params![cutoff, category, limit], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| Error::Sqlite(e))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| Error::Sqlite(e))?;
        Ok(rows)
    }

    /// 특정 카테고리 시간 버킷 집계용: (date, word, count)
    pub fn words_with_dates_category(&self, days: u64, category: &str) -> Result<Vec<(Option<String>, String, i64)>> {
        let cutoff = Self::cutoff_bound(days);
        let mut stmt = self.conn.prepare(
            "SELECT s.date, w.word, w.count FROM session_words w \
             JOIN sessions s ON s.session_id = w.session_id \
             WHERE s.date >= ?1 AND w.category = ?2"
        ).map_err(|e| Error::Sqlite(e))?;
        let rows = stmt.query_map(params![cutoff, category], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        }).map_err(|e| Error::Sqlite(e))?
          .collect::<rusqlite::Result<Vec<_>>>()
          .map_err(|e| Error::Sqlite(e))?;
        Ok(rows)
    }

    /// 시간 버킷 집계용 세션 상세: (date, session_id, total_tokens, first_user_prompt)
    pub fn session_detail_window(
        &self,
        days: u64,
    ) -> Result<Vec<(Option<String>, String, i64, Option<String>)>> {
        let cutoff = Self::cutoff_bound(days);
        let mut stmt = self.conn.prepare(
            "SELECT date, session_id, total_tokens, first_user_prompt \
             FROM sessions WHERE date >= ?1 ORDER BY date"
        ).map_err(|e| Error::Sqlite(e))?;
        let rows = stmt.query_map(params![cutoff], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        }).map_err(|e| Error::Sqlite(e))?
          .collect::<rusqlite::Result<Vec<_>>>()
          .map_err(|e| Error::Sqlite(e))?;
        Ok(rows)
    }

    /// 시간 버킷별 스킬 집계용: (date, tool_name, call_count)
    pub fn tool_usage_with_dates(&self, days: u64) -> Result<Vec<(Option<String>, String, i64)>> {
        let cutoff = Self::cutoff_bound(days);
        let mut stmt = self.conn.prepare(
            "SELECT s.date, u.tool_name, u.call_count FROM tool_usage u \
             JOIN sessions s ON s.session_id = u.session_id WHERE s.date >= ?1"
        ).map_err(|e| Error::Sqlite(e))?;
        let rows = stmt.query_map(params![cutoff], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        }).map_err(|e| Error::Sqlite(e))?
          .collect::<rusqlite::Result<Vec<_>>>()
          .map_err(|e| Error::Sqlite(e))?;
        Ok(rows)
    }

    /// 트랜잭션 시작
    pub fn begin_transaction(&self) -> Result<()> {
        self.conn.execute("BEGIN TRANSACTION", [])
            .map_err(|e| Error::Sqlite(e))?;
        Ok(())
    }

    /// 커밋
    pub fn commit(&self) -> Result<()> {
        self.conn.execute("COMMIT", [])
            .map_err(|e| Error::Sqlite(e))?;
        Ok(())
    }

    /// 롤백
    pub fn rollback(&self) -> Result<()> {
        self.conn.execute("ROLLBACK", [])
            .map_err(|e| Error::Sqlite(e))?;
        Ok(())
    }

    /// 연결 반환
    pub fn into_inner(self) -> Connection {
        self.conn
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn test_db_creation() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.sqlite");
        let db = SessionDb::new(&db_path).unwrap();
        assert_eq!(db.count_sessions().unwrap(), 0);
    }

    #[test]
    fn test_upsert_session() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.sqlite");
        let db = SessionDb::new(&db_path).unwrap();

        let meta = CodexSessionMeta {
            path: PathBuf::from("/test/rollout-2026-01-01T12-00-00-uuid.jsonl"),
            filename: "rollout-2026-01-01T12-00-00-uuid.jsonl".to_string(),
            session_id: "2026-01-01T12-00-00-uuid".to_string(),
            date: Some("2026-01-01".to_string()),
            cwd: Some("/test".to_string()),
            first_user_prompt: Some("test prompt".to_string()),
            model_provider: Some("test-provider".to_string()),
            cli_version: Some("1.0.0".to_string()),
            source: Some("test".to_string()),
            model: Some("gpt-4".to_string()),
            git_sha: Some("abc123".to_string()),
            git_branch: Some("main".to_string()),
            git_origin_url: Some("https://github.com/test".to_string()),
            tool_call_count: 10,
            file_change_count: 5,
            total_tokens: 1000,
            line_count: 100,
            corrupt_lines: 0,
            has_user_event: true,
            size_bytes: 0,
            indexed_at: Some(Utc::now()),
            tool_usage: HashMap::new(),
            word_counts: HashMap::new(),
        };

        db.upsert_session(&meta).unwrap();
        assert_eq!(db.count_sessions().unwrap(), 1);
    }

    #[test]
    fn test_tool_usage_roundtrip() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.sqlite");
        let db = SessionDb::new(&db_path).unwrap();

        let mut tools = HashMap::new();
        tools.insert("exec_command".to_string(), 12);
        tools.insert("apply_patch".to_string(), 3);

        let meta = CodexSessionMeta {
            path: PathBuf::from("/test/rollout-2026-01-01T12-00-00-uuid.jsonl"),
            filename: "rollout-2026-01-01T12-00-00-uuid.jsonl".to_string(),
            session_id: "2026-01-01T12-00-00-uuid".to_string(),
            date: Some("2026-01-01".to_string()),
            cwd: Some("/test".to_string()),
            first_user_prompt: Some("test prompt".to_string()),
            model_provider: Some("openai".to_string()),
            cli_version: Some("1.0.0".to_string()),
            source: Some("test".to_string()),
            model: Some("gpt-4".to_string()),
            git_sha: None,
            git_branch: Some("main".to_string()),
            git_origin_url: Some("https://github.com/test/repo.git".to_string()),
            tool_call_count: 15,
            file_change_count: 0,
            total_tokens: 500,
            line_count: 0,
            corrupt_lines: 0,
            has_user_event: true,
            size_bytes: 0,
            indexed_at: Some(Utc::now()),
            tool_usage: tools.clone(),
            word_counts: HashMap::new(),
        };

        db.upsert_session(&meta).unwrap();

        // 많이 쓴 순 → exec_command(12)가 apply_patch(3)보다 선행
        let top = db.top_tools(0, 10).unwrap();
        assert_eq!(top.first().map(|(n, _)| n.as_str()), Some("exec_command"));
        assert_eq!(top.len(), 2);

        // 적게 쓴 순 → apply_patch(3) 선행
        let bot = db.bottom_tools(0, 10).unwrap();
        assert_eq!(bot.first().map(|(n, _)| n.as_str()), Some("apply_patch"));

        // 재 upsert(빈 map) 시 이전 tool_usage 행 제거
        let mut meta2 = meta.clone();
        meta2.tool_usage = HashMap::new();
        db.upsert_session(&meta2).unwrap();
        assert!(db.top_tools(0, 10).unwrap().is_empty());

        // 고유 tool 수 / 프로젝트 집계
        db.upsert_session(&meta).unwrap();
        assert_eq!(db.distinct_tools(0).unwrap(), 2);
        assert_eq!(db.distinct_projects(0).unwrap(), 1);
    }

    #[test]
    fn test_session_words_roundtrip() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.sqlite");
        let db = SessionDb::new(&db_path).unwrap();

        let mut conv = HashMap::new();
        conv.insert("login".to_string(), 3);
        conv.insert("auth".to_string(), 5);
        let mut reasoning = HashMap::new();
        reasoning.insert("feasibility".to_string(), 2);
        let mut words = HashMap::new();
        words.insert("conversation".to_string(), conv);
        words.insert("reasoning".to_string(), reasoning);

        let mut meta = CodexSessionMeta {
            path: PathBuf::from("/test/rollout-2026-01-01T12-00-00-uuid.jsonl"),
            filename: "rollout-2026-01-01T12-00-00-uuid.jsonl".to_string(),
            session_id: "2026-01-01T12-00-00-uuid".to_string(),
            date: Some("2026-01-01".to_string()),
            cwd: Some("/test".to_string()),
            first_user_prompt: Some("test prompt".to_string()),
            model_provider: Some("openai".to_string()),
            cli_version: Some("1.0.0".to_string()),
            source: Some("test".to_string()),
            model: Some("gpt-4".to_string()),
            git_sha: None,
            git_branch: None,
            git_origin_url: None,
            tool_call_count: 0,
            file_change_count: 0,
            total_tokens: 0,
            line_count: 0,
            corrupt_lines: 0,
            has_user_event: true,
            size_bytes: 0,
            indexed_at: Some(Utc::now()),
            tool_usage: HashMap::new(),
            word_counts: words,
        };

        db.upsert_session(&meta).unwrap();

        // conversation: 많이 쓴 순 → auth(5)가 login(3)보다 선행
        let top = db.top_words_category(0, "conversation", 10).unwrap();
        assert_eq!(top.first().map(|(w, _)| w.as_str()), Some("auth"));
        assert_eq!(top.len(), 2);

        // reasoning 카테고리는 별도 집계
        let rtop = db.top_words_category(0, "reasoning", 10).unwrap();
        assert_eq!(rtop.first().map(|(w, _)| w.as_str()), Some("feasibility"));

        // tools 카테고리는 비어 있어야 함
        assert!(db.top_words_category(0, "tools", 10).unwrap().is_empty());

        // 재 upsert(빈 map) 시 이전 session_words 행 제거
        meta.word_counts = HashMap::new();
        db.upsert_session(&meta).unwrap();
        assert!(db.top_words_category(0, "conversation", 10).unwrap().is_empty());
    }
}
