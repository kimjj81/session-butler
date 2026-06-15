//! SQLite 데이터베이스 연산 (FTS5 포함)

use crate::error::{Error, Result};
use crate::types::CodexSessionMeta;
use chrono::Utc;
use rusqlite::{Connection, params};
use std::path::Path;

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
                indexed_at TEXT
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

        Ok(())
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
                                  line_count, corrupt_lines, has_user_event, indexed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
             ON CONFLICT(session_id) DO UPDATE SET
                path=excluded.path, date=excluded.date, cwd=excluded.cwd,
                first_user_prompt=excluded.first_user_prompt, model_provider=excluded.model_provider,
                cli_version=excluded.cli_version, source=excluded.source, model=excluded.model,
                git_sha=excluded.git_sha, git_branch=excluded.git_branch,
                git_origin_url=excluded.git_origin_url, tool_call_count=excluded.tool_call_count,
                file_change_count=excluded.file_change_count, total_tokens=excluded.total_tokens,
                line_count=excluded.line_count, corrupt_lines=excluded.corrupt_lines,
                has_user_event=excluded.has_user_event, indexed_at=excluded.indexed_at"
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
            &indexed_at,
        ]).map_err(|e| Error::Sqlite(e))?;

        // FTS5 업데이트
        let first_prompt = meta.first_user_prompt.as_deref().unwrap_or("");
        let cwd = meta.cwd.as_deref().unwrap_or("");
        let git_url = meta.git_origin_url.as_deref().unwrap_or("");

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
            indexed_at: Some(Utc::now()),
        };

        db.upsert_session(&meta).unwrap();
        assert_eq!(db.count_sessions().unwrap(), 1);
    }
}
