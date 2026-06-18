//! Codex 세션 compaction 및 민감정보 탐지.
//! (compact/sensitive-scan 은 CLI 에서 Backend::Codex 로 분류 → codex_sessions 대상.

use crate::config::Config;
use crate::error::{Error, Result};
use crate::i18n;
use crate::progress::{Progress, TerminalProgress};
use crate::types::SensitiveFile;
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use walkdir::WalkDir;

/// Codex 세션 컴팩터
pub struct SessionCompactor {
    config: Config,
    patterns: Vec<Regex>,
    progress: Arc<dyn Progress>,
}

impl SessionCompactor {
    /// 새 컴팩터 생성 (진행률 = 터미널 indicatif)
    pub fn new(config: Config) -> Result<Self> {
        let patterns = Self::default_patterns()?;
        Ok(Self { config, patterns, progress: Arc::new(TerminalProgress) })
    }

    /// 진행률 구현체 주입 (GUI: EventProgress)
    pub fn with_progress(mut self, progress: Arc<dyn Progress>) -> Self {
        self.progress = progress;
        self
    }

    /// 기본 민감정보 패턴
    fn default_patterns() -> Result<Vec<Regex>> {
        let patterns = vec![
            r#""key"\s*:\s*"sk-[a-zA-Z0-9]{20,}"#,
            r#""token"\s*:\s*"eyJ[a-zA-Z0-9_-]{20,}"#,
            r#""api_key"\s*:\s*"sk-[a-zA-Z0-9]{20,}"#,
            r#""access_token"\s*:\s*"[a-zA-Z0-9_-]{20,}"#,
            r#""secret"\s*:\s*"[a-zA-Z0-9_-]{16,}"#,
            r#""password"\s*:\s*"[^"]{8,}""#,
        ];

        patterns.into_iter()
            .map(|p| Regex::new(p).map_err(|e| Error::Other(format!("Invalid regex: {} - {}", p, e))))
            .collect()
    }

    /// trash 디렉토리(codex_archive/trash — codex_sessions 밖이라 스캐너가
    /// 휴지통 세션을 재색인하지 않음).
    fn trash_dir(&self) -> PathBuf {
        self.config.codex_archive.join("trash")
    }

    /// trash 디렉토리 생성
    pub fn create_trash_dir(&self) -> Result<()> {
        fs::create_dir_all(self.trash_dir())
            .map_err(|e| Error::Io(e))?;
        Ok(())
    }

    /// 파일명에서 날짜 추출 (rollout-YYYY-MM-DDTHH-MM-SS-uuid.jsonl -> YYYY-MM-DD)
    pub fn session_date_str(&self, path: &Path) -> Option<String> {
        let name = path.file_name()?.to_str()?;
        let rest = name.strip_prefix("rollout-")?;
        let date_part = rest.split('T').next()?;
        // YYYY-MM-DD (길이 10, [4]/[7] 이 '-')
        if date_part.len() != 10
            || date_part.as_bytes()[4] != b'-'
            || date_part.as_bytes()[7] != b'-'
            || !date_part
                .bytes()
                .all(|b| b.is_ascii_digit() || b == b'-')
        {
            return None;
        }
        Some(date_part.to_string())
    }

    /// 파일에 민감정보 포함 여부 확인
    pub fn is_sensitive_content(&self, path: &Path) -> Result<bool> {
        let content = fs::read_to_string(path)
            .map_err(|e| Error::Io(e))?;

        // 처음 50KB만 확인
        let content = content.chars().take(50_000).collect::<String>();

        for pattern in &self.patterns {
            if pattern.is_match(&content) {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// 발견된 패턴 목록 반환
    pub fn detected_patterns(&self, path: &Path) -> Result<Vec<String>> {
        let content = fs::read_to_string(path)
            .map_err(|e| Error::Io(e))?;

        let content = content.chars().take(50_000).collect::<String>();
        let mut detected = Vec::new();

        for pattern in &self.patterns {
            if pattern.is_match(&content) {
                detected.push(pattern.as_str().to_string());
            }
        }

        Ok(detected)
    }

    /// 민감정보 파일 탐지
    pub fn discover_sensitive_files(&self) -> Result<Vec<SensitiveFile>> {
        let mut results = Vec::new();

        let pb = self.progress.spinner(&i18n::scan_sensitive_progress_label());

        for entry in WalkDir::new(&self.config.codex_sessions)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            pb.inc(1);
            let path = entry.path();

            // trash 디렉토리는 건너뜀
            if path.to_string_lossy().contains("trash") {
                continue;
            }

            // JSONL 파일만 확인
            if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }

            let metadata = fs::metadata(path)
                .map_err(|e| Error::Io(e))?;
            let size_bytes = metadata.len();

            match self.detected_patterns(path) {
                Ok(patterns) if !patterns.is_empty() => {
                    results.push(SensitiveFile {
                        path: path.to_path_buf(),
                        date: self.session_date_str(path),
                        size_bytes,
                        patterns,
                    });
                }
                Err(e) => {
                    eprintln!("Error checking {}: {}", path.display(), e);
                }
                _ => {}
            }
        }
        pb.finish();

        Ok(results)
    }

    /// 세션을 trash(codex_archive/trash)로 이동
    pub fn move_to_trash(&self, path: &Path) -> Result<PathBuf> {
        let trash_dir = self.trash_dir();
        fs::create_dir_all(&trash_dir)
            .map_err(|e| Error::Io(e))?;

        let filename = path.file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| Error::InvalidPath(path.to_path_buf()))?;

        let dest = trash_dir.join(filename);

        if dest.exists() {
            // 중복 시 타임스탬프 추가
            let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
            let new_name = format!("{}.{}", filename, timestamp);
            return self.move_to_trash(&trash_dir.join(&new_name));
        }

        // trash 가 codex_archive 에 있어 codex_sessions 와 다른 파일시스템일 수 있다.
        // rename 이 EXDEV 등으로 실패하면 복사+삭제 폴백(Codex 리뷰 P2).
        if fs::rename(path, &dest).is_err() {
            fs::copy(path, &dest).map_err(|e| Error::Io(e))?;
            fs::remove_file(path).map_err(|e| Error::Io(e))?;
        }

        Ok(dest)
    }

    /// 오늘 이전의 세션만 필터링
    pub fn filter_old_sessions(&self, days: u64) -> Result<Vec<PathBuf>> {
        let cutoff = chrono::Utc::now().date_naive() - chrono::Days::new(days);
        let mut old_sessions = Vec::new();

        for entry in WalkDir::new(&self.config.codex_sessions)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();

            if path.to_string_lossy().contains("trash") {
                continue;
            }

            if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }

            if let Some(date_str) = self.session_date_str(path) {
                if let Ok(date) = chrono::NaiveDate::parse_from_str(&date_str, "%Y-%m-%d") {
                    if date < cutoff {
                        old_sessions.push(path.to_path_buf());
                    }
                }
            }
        }

        Ok(old_sessions)
    }

    /// 세션 compaction 수행 (안전하게 - 원본은 trash로 이동만)
    pub fn compact_sessions(&self, days: u64, dry_run: bool) -> Result<CompactionResult> {
        self.create_trash_dir()?;

        let old_sessions = self.filter_old_sessions(days)?;
        let mut moved = Vec::new();
        let mut skipped = Vec::new();

        let pb = self.progress.bar(old_sessions.len() as u64, &i18n::compact_progress_label());

        for path in &old_sessions {
            pb.inc(1);
            if dry_run {
                println!("Would move {} to trash", path.display());
                skipped.push(path.clone());
                continue;
            }

            match self.move_to_trash(path) {
                Ok(dest) => {
                    println!("Moved {} -> {}", path.display(), dest.display());
                    moved.push((path.clone(), dest));
                }
                Err(e) => {
                    eprintln!("Error moving {}: {}", path.display(), e);
                    skipped.push(path.clone());
                }
            }
        }
        pb.finish();

        Ok(CompactionResult {
            moved,
            skipped,
            total: old_sessions.len(),
        })
    }
}

/// Compaction 결과
#[derive(Debug)]
pub struct CompactionResult {
    pub moved: Vec<(PathBuf, PathBuf)>,
    pub skipped: Vec<PathBuf>,
    pub total: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs::{self, File};
    use std::io::Write;

    #[test]
    fn test_session_date_str() {
        let config = Config::default();
        let compactor = SessionCompactor::new(config).unwrap();

        let path = Path::new("rollout-2026-05-24T08-50-35-abc.jsonl");
        assert_eq!(compactor.session_date_str(path), Some("2026-05-24".to_string()));

        let path = Path::new("invalid.jsonl");
        assert_eq!(compactor.session_date_str(path), None);
    }

    #[test]
    fn test_is_sensitive_content() {
        let dir = TempDir::new().unwrap();
        let test_file = dir.path().join("test.jsonl");

        let mut file = File::create(&test_file).unwrap();
        writeln!(file, r#"{{"key": "sk-12345678901234567890"}}"#).unwrap();

        let config = Config::default();
        let compactor = SessionCompactor::new(config).unwrap();

        assert!(compactor.is_sensitive_content(&test_file).unwrap());

        // 일반 콘텐츠
        let normal_file = dir.path().join("normal.jsonl");
        let mut file = File::create(&normal_file).unwrap();
        writeln!(file, r#"{{"message": "Hello, World!"}}"#).unwrap();

        assert!(!compactor.is_sensitive_content(&normal_file).unwrap());
    }

    #[test]
    fn test_detected_patterns() {
        let dir = TempDir::new().unwrap();
        let test_file = dir.path().join("test.jsonl");

        let mut file = File::create(&test_file).unwrap();
        writeln!(file, r#"{{"key": "sk-12345678901234567890", "token": "eyJ0eXAiOiJKV1QiLCJhbGc"}}}}"#).unwrap();

        let config = Config::default();
        let compactor = SessionCompactor::new(config).unwrap();

        let patterns = compactor.detected_patterns(&test_file).unwrap();
        assert!(!patterns.is_empty());
        assert!(patterns.iter().any(|p| p.contains("key")));
    }

    #[test]
    fn test_move_to_trash() {
        let dir = TempDir::new().unwrap();
        let sessions_dir = dir.path().join("sessions");
        fs::create_dir_all(&sessions_dir).unwrap();

        let test_file = sessions_dir.join("rollout-2026-01-01T00-00-00-x.jsonl");
        File::create(&test_file).unwrap();

        let mut config = Config::default();
        config.codex_sessions = sessions_dir.clone();
        config.codex_archive = dir.path().join("archive");

        let compactor = SessionCompactor::new(config).unwrap();
        let trash_path = compactor.move_to_trash(&test_file).unwrap();

        assert!(trash_path.exists());
        assert!(!test_file.exists());
        assert!(trash_path.to_string_lossy().contains("trash"));
    }
}
