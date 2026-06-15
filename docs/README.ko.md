# Session Butler

> 🌐 언어: [English](../README.md) · **한국어**

**Codex** / **Hermes** 세션 기록을 압축·보관하고, 검색 가능한 지식베이스로 만들어주는 도구.

---

## 왜 만들었는가

**Codex**와 **Hermes**는 대화 내용을 JSONL/JSON으로 홈 디렉토리 밑에 조용히 저장한다. 매일 쓰다 보면 몇 달 만에 파일이 불어나는데, 제 경우 `~/.codex/sessions/`는 **3 GB**를 넘겼다.

이 기록은 분명 귀중하다. 몇 달 치의 디버깅 노트, 설계 결정, 공들여 다듬은 프롬프트가 고스란히 들어있다. 하지만 3 GB짜리 파일은 디스크에 둔 채로는 너무 무겁고, raw 그대로 뒤지기엔 고통스럽고, 그렇다고 지우기엔 아깝다. 결국 자리만 차지하고 아무 쓸모가 없는 **죽은 무게**가 되어 있었다.

**Session Butler는 이 딜레마를 푼다.** 오래된 세션의 용량을 줄이는 동시에, 검색하고 다시 꺼내 쓸 수 있는 개인 지식베이스로 바꿔서 폴더 속에서 썩히지 않게 만든다.

## 목표

하나의 안전한 파이프라인으로:

1. **인덱싱** — 모든 세션을 전문검색(FTS5) 지원 SQLite에 색인.
2. **압축** — 오래된 세션을 zstd로 압축. 원본은 자동 삭제하지 않음.
3. **컴팩션** — 세션을 정리하고 민감정보(API key, token 등)를 탐지.
4. **요약** — 세션을 검색 가능한 요약 + 키워드 레이어로 변환.

…그래서 몇 달 치 AI 작업 기록이 디스크에서는 작게, 손 안에서는 쓸모 있게 남도록.

## 동작 방식 — 4단계 파이프라인

| 단계 | 명령 | 하는 일 |
|------|------|---------|
| 1 · Scan | `scan` | Codex `rollout-*.jsonl`을 순회하며 메타데이터 + FTS5 인덱스를 SQLite에 저장 |
| 2 · Archive | `archive` / `restore` / `list` / `stats` | N일 이상 지난 세션을 zstd 압축, SHA-256 체크섬 기록 |
| 3 · Compact | `compact` | 안전한 컴팩션 + 민감정보 탐지(`.env`, token, key) |
| 4 · Summarize | `summarize` | Hermes 세션 분석 → 요약 + FTS5 키워드 JSON 생성 |

## 빌드

Rust(edition 2024)가 필요하다.

```bash
cargo build --release
# → ./target/release/session-butler
```

개발 중에는 그냥 실행해도 된다:

```bash
cargo run --release -- <명령>
```

## 사용법

### 인터랙티브 (TUI)

```bash
session-butler          # 인자 없음 → TUI 실행
session-butler --tui    # 명시적 실행
```

TUI는 4단계 전체(Scan, Archive, Restore, List, Stats, Compact, Summarize, Pipeline)를 하나의 메뉴로 묶고 인자를 직접 편집할 수 있어서, 일회성 실행에 편하다.

### CLI

```bash
# Phase 1 — Codex 세션 스캔 + 인덱싱
session-butler scan [--analyze]

# Phase 2 — 압축
session-butler archive --days 30 --dry-run   # 미리보기
session-butler archive --days 30             # 압축 (원본 보존)
session-butler restore --all                 # 전체 복원
session-butler list   --days 30 [--json]
session-butler stats  --days 30

# Phase 3 — 컴팩션
session-butler compact --scan-sensitive      # 민감정보 스캔만
session-butler compact --days 0 --dry-run    # 컴팩션 미리보기

# Phase 4 — Hermes 세션 요약
session-butler summarize                     # 요약 + FTS5 JSON
session-butler summarize --summary-only
session-butler summarize --fts-only

# 전체 파이프라인 순차 실행
session-butler pipeline --days 30 --dry-run
```

### 공용 옵션 (모든 명령)

| 플래그 | 기본값 |
|------|---------|
| `-C, --codex-sessions <DIR>` | `~/.codex/sessions` |
| `-H, --hermes-sessions <DIR>` | `~/.hermes/sessions` |
| `-A, --codex-archive <DIR>` | `~/.codex/archive` |
| `-I, --codex-index-db <PATH>` | `./codex_index.sqlite` |
| `-S, --summary-layer <PATH>` | `./summary_layer.json` |
| `-F, --fts5-index <PATH>` | `./fts5_index.json` |
| `-v, --verbose` | 상세 출력 |

### 환경변수

`CODEX_SESSIONS`, `CODEX_ARCHIVE`, `HERMES_SESSIONS`, `CODEX_STATE_DB`, `CODEX_INDEX_DB`, `SUMMARY_LAYER_JSON`, `FTS5_INDEX_JSON` — 위 플래그와 동일한 의미/기본값.

## 결과

실제 제 세션 기록으로 측정한 수치:

| 대상 | 파일 수 | 원본 크기 | 결과 |
|------|--------:|---------:|-------|
| Codex 세션 | 3,037 | 3.1 GB | 압축 대상(2,303개) **2.42 GB → 0.86 GB** (약 64% 축소) |
| Hermes 세션 | 82 | 47 MB | 요약 → `summary_layer.json` + `fts5_index.json` |

즉, **2.4 GB짜리 과거 Codex 세션이 약 860 MB로 줄었고**, SQLite + FTS5 인덱스로 전문검색도 그대로 가능하다. 압축 후 원본을 지우면 그만큼 디스크를 추가로 확보할 수 있다.

## 안전성

- 어떤 단계에서도 원본 세션 파일을 **자동 삭제하지 않는다**. 압축은 `.jsonl.zst` 사본을 만들 뿐이며, 원본은 명시적으로 restore/quarantine할 때만 옮겨진다.
- 보존 기간(`--days`, 기본 `30`)보다 최근 세션은 대상에서 제외된다.
- 압축 파일마다 SHA-256 체크섬을 `checksums.jsonl`에 기록하므로 `restore` 시 무결성을 검증할 수 있다.

## 개발 현황

원래 Python 프로토타입을 Rust로 포팅. 4단계 파이프라인, TUI, CLI 모두 동작한다. Phase 1·4 출력은 Python 원본과 일치함을 검증했고, Phase 2는 충실하게 따른다.

## 라이선스

[MIT](../LICENSE) © Kim Jeongjin
