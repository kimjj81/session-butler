//! Phase 2: Codex 세션 압축 (zstd) 및 관리

use crate::config::Config;
use crate::error::{Error, Result};
use crate::types::ArchivedSession;
use chrono::Datelike;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use walkdir::WalkDir;
use zstd::stream::Encoder;

const CHECKSUM_FILE: &str = "checksums.jsonl";

/// Codex 세션 아카이버
pub struct SessionArchiver {
    config: Config,
    compression_level: i32,
}

impl SessionArchiver {
    /// 새 아카이버 생성
    pub fn new(config: Config) -> Self {
        Self {
            config,
            compression_level: 3, // zstd level 3 for speed/good ratio
        }
    }

    /// 압축 레벨 설정
    pub fn with_compression_level(mut self, level: i32) -> Self {
        self.compression_level = level.clamp(1, 22);
        self
    }

    /// 모든 세션 발견
    pub fn discover_sessions(&self) -> Result<Vec<SessionInfo>> {
        let mut sessions = Vec::new();

        for entry in WalkDir::new(&self.config.codex_sessions)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }

            let filename = path.file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| Error::InvalidPath(path.to_path_buf()))?;

            if !filename.starts_with("rollout-") {
                continue;
            }

            let date = self.parse_date_from_filename(filename);
            let metadata = fs::metadata(path)
                .map_err(|e| Error::Io(e))?;
            let size_bytes = metadata.len();

            // 첫 줄에서 메타데이터 추출
            let mut session_id = None;
            let mut model_provider = None;
            let mut cli_version = None;

            if let Ok(mut file) = File::open(path) {
                let mut first_line = String::new();
                if file.read_to_string(&mut first_line).is_ok() {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&first_line) {
                        if let Some(payload) = value.get("payload") {
                            session_id = payload.get("id").and_then(|v| v.as_str()).map(String::from);
                            model_provider = payload.get("model_provider").and_then(|v| v.as_str()).map(String::from);
                            cli_version = payload.get("cli_version").and_then(|v| v.as_str()).map(String::from);
                        }
                    }
                }
            }

            sessions.push(SessionInfo {
                path: path.to_path_buf(),
                date,
                size_bytes,
                session_id,
                model_provider,
                cli_version,
            });
        }

        Ok(sessions)
    }

    /// 파일명에서 날짜 추출
    fn parse_date_from_filename(&self, filename: &str) -> Option<chrono::NaiveDate> {
        // rollout-2026-03-03T14-37-44-...jsonl
        let parts: Vec<&str> = filename.split('T').collect();
        if parts.len() < 2 {
            return None;
        }

        let date_str = parts[0].strip_prefix("rollout-")?;
        FromStr::from_str(date_str).ok()
    }

    /// 날짜 기준 필터링
    pub fn filter_by_days<'a>(&self, sessions: &'a [SessionInfo], days: u64) -> Vec<&'a SessionInfo> {
        let cutoff = chrono::Utc::now().date_naive() - chrono::Days::new(days);

        sessions.iter()
            .filter(|s| s.date.map_or(false, |d| d >= cutoff))
            .collect()
    }

    /// 세션 압축
    pub fn archive(&self, sessions: &[&SessionInfo], dry_run: bool) -> Result<ArchiveResult> {
        let dest = &self.config.codex_archive;
        fs::create_dir_all(dest)
            .map_err(|e| Error::Io(e))?;

        let mut archived = Vec::new();
        let mut skipped = Vec::new();
        let mut total_original = 0u64;
        let mut total_compressed = 0u64;

        for session in sessions {
            let src = &session.path;

            // 대상 경로 계산
            let dest_path = if let Some(date) = session.date {
                let relative = src.strip_prefix(&self.config.codex_sessions)
                    .unwrap_or(src);

                // 연도 제거 (sessions/2026/06/03 -> 06/03)
                let parts: Vec<&str> = relative.components()
                    .filter_map(|c| c.as_os_str().to_str())
                    .collect();

                let date_path = if parts.len() >= 3 {
                    // 월/일/파일 형태로 변환
                    format!("{}/{}/{}", date.month(), date.day(), src.file_name().unwrap().to_string_lossy())
                } else {
                    src.file_name().unwrap().to_string_lossy().to_string()
                };

                dest.join(date.to_string()).join(date_path)
            } else {
                dest.join(src.file_name().unwrap().to_string_lossy().as_ref())
            };

            dest_path.parent().map(|p| fs::create_dir_all(p))
                .transpose()
                .map_err(|e| Error::Io(e))?;

            let zst_path = PathBuf::from(format!("{}.zst", dest_path.display()));

            if dry_run {
                skipped.push((**session).clone());
                continue;
            }

            // 압축 실행
            match self.compress_file(src, &zst_path) {
                Ok(compressed_size) => {
                    let checksum = self.sha256_file(src)?;

                    archived.push(ArchivedSession {
                        original: src.clone(),
                        compressed: zst_path.clone(),
                        checksum_sha256: checksum,
                        date: session.date.map(|d| d.to_string()),
                        size_bytes: session.size_bytes,
                        compressed_size_bytes: compressed_size,
                    });

                    total_original += session.size_bytes;
                    total_compressed += compressed_size;
                }
                Err(e) => {
                    eprintln!("ERROR compressing {}: {}", src.display(), e);
                    skipped.push((**session).clone());
                }
            }
        }

        // 체크섬 파일 작성
        if !dry_run && !archived.is_empty() {
            let checksum_path = dest.join(CHECKSUM_FILE);
            let mut file = File::create(&checksum_path)
                .map_err(|e| Error::Io(e))?;

            for entry in &archived {
                let line = serde_json::to_string(entry)
                    .map_err(|e| Error::Json(e))?;
                writeln!(file, "{}", line)
                    .map_err(|e| Error::Io(e))?;
            }
        }

        // 요약 출력
        let ratio = if total_original > 0 {
            (1.0 - (total_compressed as f64 / total_original as f64)) * 100.0
        } else {
            0.0
        };

        println!("Archived {} sessions ({:.1}GB -> {:.1}GB, {:.0}% reduction)",
            archived.len(),
            total_original as f64 / (1024.0 * 1024.0 * 1024.0),
            total_compressed as f64 / (1024.0 * 1024.0 * 1024.0),
            ratio
        );

        if !skipped.is_empty() {
            println!("Skipped: {} sessions", skipped.len());
        }

        Ok(ArchiveResult {
            archived,
            skipped,
            total_original,
            total_compressed,
        })
    }

    /// 파일 압축
    fn compress_file(&self, src: &Path, dest: &Path) -> Result<u64> {
        let src_file = File::open(src)
            .map_err(|e| Error::Io(e))?;
        let dest_file = File::create(dest)
            .map_err(|e| Error::Io(e))?;

        let mut encoder = Encoder::new(dest_file, self.compression_level)
            .map_err(|e| Error::Compression(e.to_string()))?;
        let mut reader = io::BufReader::new(src_file);

        io::copy(&mut reader, &mut encoder)
            .map_err(|e| Error::Io(e))?;

        let mut output = encoder.finish()
            .map_err(|e| Error::Compression(e.to_string()))?;

        output.flush()
            .map_err(|e| Error::Io(e))?;

        Ok(fs::metadata(dest)
            .map_err(|e| Error::Io(e))?
            .len())
    }

    /// 파일 압축 해제
    pub fn decompress_file(&self, src: &Path, dest: &Path) -> Result<()> {
        let src_file = File::open(src)
            .map_err(|e| Error::Io(e))?;
        let dest_file = File::create(dest)
            .map_err(|e| Error::Io(e))?;

        let reader = io::BufReader::new(src_file);
        let mut writer = io::BufWriter::new(dest_file);

        // zstd decoder 사용
        let mut decoder = zstd::stream::Decoder::new(reader)
            .map_err(|e| Error::Compression(e.to_string()))?;

        io::copy(&mut decoder, &mut writer)
            .map_err(|e| Error::Io(e))?;

        writer.flush()
            .map_err(|e| Error::Io(e))?;

        Ok(())
    }

    /// SHA256 체크섬 계산
    fn sha256_file(&self, path: &Path) -> Result<String> {
        use sha2::{Digest, Sha256};

        let mut file = File::open(path)
            .map_err(|e| Error::Io(e))?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 65536];

        loop {
            let n = file.read(&mut buffer)
                .map_err(|e| Error::Io(e))?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }

        Ok(format!("{:x}", hasher.finalize()))
    }

    /// 세션 복원
    pub fn restore(&self, sessions: &[&SessionInfo], dry_run: bool) -> Result<Vec<SessionInfo>> {
        let mut restored = Vec::new();

        for session in sessions {
            let src = &session.path;

            // .zst 경로 계산
            let zst_path = if let Some(date) = session.date {
                let date_path = format!("{}/{}/{}.zst",
                    date.month(),
                    date.day(),
                    src.file_name().unwrap().to_string_lossy()
                );

                self.config.codex_archive.join(date.to_string()).join(date_path)
            } else {
                self.config.codex_archive.join(format!("{}.zst",
                    src.file_name().unwrap().to_string_lossy()
                ))
            };

            if !zst_path.exists() {
                eprintln!("WARNING: {} not found, skipping", zst_path.display());
                continue;
            }

            if dry_run {
                println!("  restore {} -> {}", zst_path.display(), src.display());
                restored.push((**session).clone());
                continue;
            }

            match self.decompress_file(&zst_path, src) {
                Ok(_) => restored.push((**session).clone()),
                Err(e) => eprintln!("ERROR restoring {}: {}", zst_path.display(), e),
            }
        }

        println!("Restored {} sessions", restored.len());
        Ok(restored)
    }

    /// 세션 목록 출력
    pub fn list_sessions(&self, sessions: &[SessionInfo], as_json: bool) -> Result<()> {
        if as_json {
            println!("{}", serde_json::to_string_pretty(sessions)
                .map_err(|e| Error::Json(e))?);
        } else {
            // 날짜별 그룹화
            let mut by_date: std::collections::BTreeMap<String, Vec<&SessionInfo>> = std::collections::BTreeMap::new();

            for session in sessions {
                let key = session.date.map(|d| d.to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                by_date.entry(key).or_default().push(session);
            }

            for (date, items) in by_date {
                let total_size: u64 = items.iter().map(|s| s.size_bytes).sum();
                println!("\n{}: {} sessions, {:.1}MB",
                    date,
                    items.len(),
                    total_size as f64 / (1024.0 * 1024.0)
                );

                for session in items.iter().take(5) {
                    println!("  {} ({:.0}KB) model={}",
                        session.path.file_name().unwrap().to_string_lossy(),
                        session.size_bytes as f64 / 1024.0,
                        session.model_provider.as_deref().unwrap_or("?")
                    );
                }

                if items.len() > 5 {
                    println!("  ... and {} more", items.len() - 5);
                }
            }
        }

        Ok(())
    }

    /// 통계 출력
    pub fn show_stats(&self, sessions: &[SessionInfo]) -> Result<()> {
        if sessions.is_empty() {
            println!("No sessions found.");
            return Ok(());
        }

        let total_size: u64 = sessions.iter().map(|s| s.size_bytes).sum();

        let mut by_provider: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        let mut by_month: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

        for session in sessions {
            let provider = session.model_provider.as_deref().unwrap_or("unknown");
            *by_provider.entry(provider.to_string()).or_default() += 1;

            let month = session.date.map(|d| format!("{}-{:02}", d.year(), d.month()))
                .unwrap_or_else(|| "unknown".to_string());
            *by_month.entry(month).or_default() += 1;
        }

        println!("Sessions: {}", sessions.len());
        println!("Total size: {:.2} GB", total_size as f64 / (1024.0 * 1024.0 * 1024.0));
        println!("\nBy provider:");

        let mut provider_vec: Vec<_> = by_provider.iter().collect();
        provider_vec.sort_by(|a, b| b.0.cmp(a.0));
        for (provider, count) in provider_vec {
            println!("  {}: {}", provider, count);
        }

        println!("\nBy month:");
        let mut month_vec: Vec<_> = by_month.iter().collect();
        month_vec.sort_by(|a, b| a.0.cmp(b.0));
        for (month, count) in month_vec {
            println!("  {}: {}", month, count);
        }

        Ok(())
    }
}

/// 세션 정보
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionInfo {
    pub path: PathBuf,
    pub date: Option<chrono::NaiveDate>,
    pub size_bytes: u64,
    pub session_id: Option<String>,
    pub model_provider: Option<String>,
    pub cli_version: Option<String>,
}

/// 아카이브 결과
#[derive(Debug)]
pub struct ArchiveResult {
    pub archived: Vec<ArchivedSession>,
    pub skipped: Vec<SessionInfo>,
    pub total_original: u64,
    pub total_compressed: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs::write;

    #[test]
    fn test_parse_date_from_filename() {
        let config = Config::default();
        let archiver = SessionArchiver::new(config);

        let date = archiver.parse_date_from_filename("rollout-2026-03-03T14-37-44-uuid.jsonl");
        assert_eq!(date, Some(chrono::NaiveDate::from_ymd_opt(2026, 3, 3).unwrap()));

        let date = archiver.parse_date_from_filename("invalid.jsonl");
        assert_eq!(date, None);
    }

    #[test]
    fn test_compress_decompress() {
        let dir = TempDir::new().unwrap();
        let src_file = dir.path().join("test.txt");
        let zst_file = dir.path().join("test.txt.zst");
        let restored_file = dir.path().join("restored.txt");

        let test_data = b"Hello, World! This is a test for compression.";
        write(&src_file, test_data).unwrap();

        let config = Config::default();
        let archiver = SessionArchiver::new(config);

        archiver.compress_file(&src_file, &zst_file).unwrap();
        assert!(zst_file.exists());

        archiver.decompress_file(&zst_file, &restored_file).unwrap();

        let restored = fs::read_to_string(&restored_file).unwrap();
        assert_eq!(restored, String::from_utf8_lossy(test_data));
    }

    #[test]
    fn test_sha256() {
        let dir = TempDir::new().unwrap();
        let test_file = dir.path().join("test.txt");

        write(&test_file, b"test data").unwrap();

        let config = Config::default();
        let archiver = SessionArchiver::new(config);

        let checksum = archiver.sha256_file(&test_file).unwrap();
        assert_eq!(checksum, "916f0027a575074ce72a331777c3478d6513f786a591bd892da1a577bf2335f9");
    }
}
