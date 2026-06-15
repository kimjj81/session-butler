//! Phase 2: Codex 세션 압축 (zstd) 및 관리

use crate::config::Config;
use crate::db::SessionDb;
use crate::error::{Error, Result};
use crate::types::{ArchivedSession, ArchivedSessionRow, SessionInfo};
use chrono::Datelike;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use walkdir::WalkDir;
use zstd::stream::Encoder;

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

    /// 세션의 .zst 보관본 대상 경로 계산 (archive 전용).
    /// restore는 이 함수 대신 DB의 compressed_path를 직접 사용한다.
    fn zst_dest_path(&self, session: &SessionInfo) -> PathBuf {
        let src = &session.path;
        let dest = &self.config.codex_archive;

        let dest_path = if let Some(date) = session.date {
            let relative = src.strip_prefix(&self.config.codex_sessions)
                .unwrap_or(src);

            // 연도 제거 (sessions/2026/06/03 -> 06/03)
            let parts: Vec<&str> = relative.components()
                .filter_map(|c| c.as_os_str().to_str())
                .collect();

            let date_path = if parts.len() >= 3 {
                // 월/일/파일 형태로 변환
                format!("{}/{}/{}", date.month(), date.day(),
                    src.file_name().unwrap().to_string_lossy())
            } else {
                src.file_name().unwrap().to_string_lossy().to_string()
            };

            dest.join(date.to_string()).join(date_path)
        } else {
            dest.join(src.file_name().unwrap().to_string_lossy().as_ref())
        };

        PathBuf::from(format!("{}.zst", dest_path.display()))
    }

    /// 세션 압축
    pub fn archive(&self, sessions: &[&SessionInfo], dry_run: bool, move_originals: bool, db: &SessionDb) -> Result<ArchiveResult> {
        let dest = &self.config.codex_archive;
        fs::create_dir_all(dest)
            .map_err(|e| Error::Io(e))?;

        let mut archived = Vec::new();
        let mut skipped = Vec::new();
        let mut total_original = 0u64;
        let mut total_compressed = 0u64;

        for session in sessions {
            let src = &session.path;
            let zst_path = self.zst_dest_path(session);

            if dry_run {
                skipped.push((**session).clone());
                continue;
            }

            // 대상 디렉토리 생성 (dry-run이 아닐 때만)
            zst_path.parent().map(|p| fs::create_dir_all(p))
                .transpose()
                .map_err(|e| Error::Io(e))?;

            // 압축 실행
            match self.compress_file(src, &zst_path) {
                Ok(compressed_size) => {
                    let checksum = self.sha256_file(src)?;

                    // DB에 archived 상태 기록 (session_id 필수)
                    if let Some(ref sid) = session.session_id {
                        if let Err(e) = db.mark_archived(sid, &zst_path, &checksum) {
                            eprintln!("ERROR marking archived {}: {}", src.display(), e);
                        }
                    } else {
                        eprintln!("WARNING: {} session_id 없음, DB 상태 기록 생략", src.display());
                    }

                    // --move: 압축+DB 기록 성공 후에만 원본 삭제
                    if move_originals {
                        if let Err(e) = fs::remove_file(src) {
                            eprintln!("WARNING: 원본 삭제 실패 {}: {}", src.display(), e);
                        }
                    }

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
        // 임시 파일에 해제한 뒤 rename — 실패해도 dest(원본 자리)가 truncate되지 않도록
        let tmp = PathBuf::from(format!("{}.part", dest.display()));

        let result = (|| -> Result<()> {
            let src_file = File::open(src)
                .map_err(|e| Error::Io(e))?;
            let tmp_file = File::create(&tmp)
                .map_err(|e| Error::Io(e))?;

            let reader = io::BufReader::new(src_file);
            let mut writer = io::BufWriter::new(tmp_file);

            // zstd decoder 사용
            let mut decoder = zstd::stream::Decoder::new(reader)
                .map_err(|e| Error::Compression(e.to_string()))?;

            io::copy(&mut decoder, &mut writer)
                .map_err(|e| Error::Io(e))?;

            writer.flush()
                .map_err(|e| Error::Io(e))?;

            Ok(())
        })();

        match result {
            Ok(()) => {
                fs::rename(&tmp, dest).map_err(|e| Error::Io(e))?;
                Ok(())
            }
            Err(e) => {
                // 실패 시 임시 파일 정리 (dest는 건드리지 않음)
                let _ = fs::remove_file(&tmp);
                Err(e)
            }
        }
    }

    /// SHA256 체크섬 계산
    pub(crate) fn sha256_file(&self, path: &Path) -> Result<String> {
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

    /// 세션 복원 (DB의 archived 세션을 대상).
    /// discover_sessions()에 의존하지 않고 ArchivedSessionRow(compressed_path)를 직접 사용.
    pub fn restore(&self, rows: &[ArchivedSessionRow], dry_run: bool, purge: bool, db: &SessionDb) -> Result<Vec<ArchivedSessionRow>> {
        let mut restored = Vec::new();

        for row in rows {
            let zst = &row.compressed_path;
            let dest = &row.path;

            if !zst.exists() {
                eprintln!("WARNING: 보관본 없음, skip: {}", zst.display());
                continue;
            }

            if dry_run {
                println!("  restore {} -> {}", zst.display(), dest.display());
                restored.push(row.clone());
                continue;
            }

            // 복원 대상 디렉토리 보장 (--move로 원본과 함께 디렉토리가 사라졌을 수 있음)
            if let Some(parent) = dest.parent() {
                if let Err(e) = fs::create_dir_all(parent) {
                    eprintln!("ERROR 복원 디렉토리 생성 실패 {}: {}", dest.display(), e);
                    continue;
                }
            }

            match self.decompress_file(zst, dest) {
                Ok(_) => {
                    // 무결성 검증 (체크섬이 있을 때)
                    if !row.checksum_sha256.is_empty() {
                        match self.sha256_file(dest) {
                            Ok(actual) if actual == row.checksum_sha256 => {}
                            Ok(actual) => {
                                eprintln!("WARNING 체크섬 불일치 {}: expected {} got {}",
                                    dest.display(), row.checksum_sha256, actual);
                            }
                            Err(e) => eprintln!("WARNING 체크섬 계산 실패 {}: {}", dest.display(), e),
                        }
                    }

                    // purge: 보관본 삭제 + DB archived 해제
                    if purge {
                        if let Err(e) = fs::remove_file(zst) {
                            eprintln!("WARNING 보관본 삭제 실패 {}: {}", zst.display(), e);
                        }
                        if let Err(e) = db.mark_purged(&row.session_id) {
                            eprintln!("ERROR DB purge 표시 실패 {}: {}", row.session_id, e);
                        }
                    }

                    restored.push(row.clone());
                }
                Err(e) => eprintln!("ERROR restoring {}: {}", zst.display(), e),
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
