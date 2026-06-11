# session-butler

Codex/Hermes 세션 파일 관리 도구.

~/.codex/sessions/ (Codex) 와 ~/.hermes/sessions/ (Hermes) 에 쌓이는
세션 JSONL/JSON 파일을 스캔, 압축, compaction, 요약하는 4단계 파이프라인.

## 구성

| 파일 | 단계 | 용도 |
|------|------|------|
| `codex_scanner.py` | Phase 1 | Codex rollout-\*.jsonl 스캔 + SQLite 인덱싱 |
| `archive.py` | Phase 2 | zstd 압축, checksum, 복원 |
| `compact.py` | Phase 3 | 안전한 compaction + 민감정보 탐지 |
| `summary_layer.py` | Phase 4 | Hermes 세션 분석 (라이브러리) |
| `write_summary_layer.py` | Phase 4 | 분석 결과를 JSON으로 기록 |

산출물:
- `codex_index.sqlite` — 2834개 Codex 세션 메타데이터 + FTS5
- `summary_layer.json` — 52개 Hermes 세션 요약
- `fts5_index.json` — FTS5 키워드 인덱스

## 현재 처리 현황

| 대상 | 파일 수 | 원본 크기 | 압축 후 |
|------|---------|----------|---------|
| Codex 세션 | 2834 | 3.0 GB | 828 MB (archive) |
| Hermes 세션 | 52 | 46 MB | — |

## 사용법

### Phase 1: Codex 세션 스캔

```bash
# 기본 경로(~/.codex/sessions) 스캔
python3 codex_scanner.py

# 환경변수로 경로 지정
CODEX_SESSIONS=/path/to/sessions CODEX_INDEX_DB=./my_index.sqlite python3 codex_scanner.py
```

환경변수:
- `CODEX_SESSIONS` — 세션 디렉토리 (기본: `~/.codex/sessions`)
- `CODEX_STATE_DB` — Codex state\_5.sqlite 경로 (비교용)
- `CODEX_INDEX_DB` — 출력 SQLite 경로 (기본: `./codex_index.sqlite`)

출력: 월별/프로젝트별/모델별 분석 + state\_5.sqlite 비교 + FTS5 검색 데모.

### Phase 2: Codex 세션 압축

```bash
# 30일 이전 세션 압축 (dry-run)
python3 archive.py archive --days 30 --dry-run

# 실제 압축 실행
python3 archive.py archive --days 30

# 압축 해제 (전체)
python3 archive.py restore --all

# 압축 목록 조회
python3 archive.py list

# 통계
python3 archive.py stats
```

환경변수:
- `CODEX_SESSIONS` — 원본 디렉토리 (기본: `~/.codex/sessions`)
- `CODEX_ARCHIVE` — 압축 파일 저장 위치 (기본: `~/.codex/archive`)

압축 결과:
- `.jsonl` → `.jsonl.zst` (zstd level 3)
- checksums.jsonl에 SHA256 기록
- 원본 파일 보존 (자동 삭제 안 함)

### Phase 3: Compaction

```python
# compact.py는 라이브러리 형태. 주요 함수:
from compact import discover_sensitive_files, create_trash_dir

# 민감정보 스캔 (.env, token, key 패턴)
results = discover_sensitive_files()
```

주요 함수:
- `discover_sensitive_files()` — 세션 파일에서 API key, token 등 패턴 탐지
- `create_trash_dir()` — trash 디렉토리 생성
- `session_date_str(path)` — 파일명에서 날짜 추출
- `is_sensitive_content(path)` — 민감정보 포함 여부 확인

### Phase 4: Hermes 세션 요약

```bash
# 분석 + JSON 출력
python3 write_summary_layer.py

# 경로 지정
python3 write_summary_layer.py \
  --sessions-dir ~/.hermes/sessions \
  --summary-path ./summary_layer.json \
  --fts-path ./fts5_index.json
```

summary\_layer.json 구조:
```json
{
  "schema_version": 1,
  "generated_at": "2026-06-11T...",
  "sessions_dir": "/Users/.../.hermes/sessions",
  "session_count": 52,
  "sessions": [
    {
      "session_id": "...",
      "model": "GLM-5.1",
      "title": "discord 연결 확인",
      "first_user_prompt": "...",
      "summary": "...",
      "keywords": ["discord", "연결"],
      "tool_usage": {"terminal": 5, "read_file": 3},
      "project_context": []
    }
  ]
}
```

## 인덱스 DB 쿼리 예시

```bash
# 세션 검색 (FTS5)
sqlite3 codex_index.sqlite \
  "SELECT session_id, date, cwd FROM sessions_fts WHERE sessions_fts MATCH 'myven' LIMIT 10"

# 월별 세션 수
sqlite3 codex_index.sqlite \
  "SELECT substr(date,1,7) AS m, COUNT(*) FROM sessions GROUP BY m ORDER BY m"

# 특정 프로젝트 세션
sqlite3 codex_index.sqlite \
  "SELECT session_id, date, line_count FROM sessions WHERE cwd LIKE '%myven%' ORDER BY date"
```

## 주의사항

- 원본 세션 파일은 어떤 단계에서도 자동 삭제하지 않음
- 압축 후 원본은 그대로 유지, trash/quarantine으로 이동만 수행
- 최근 30일 세션은 archive/compact 대상에서 제외
- compact.py는 main 함수가 없어 필요한 함수를 import해서 사용
