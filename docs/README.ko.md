# Session Butler

> 🌐 언어: [English](../README.md) · **한국어**

세션 기록을 압축·보관하고, 검색 가능한 지식베이스로 만들어주는 도구.

- **Codex** 세션 → 스캔/인덱싱, 압축/보관(+복원), 컴팩션 및 민감정보 탐지.
- **요약 백엔드**(`session_*.json`) → 요약 + 키워드 검색 레이어 생성.
- 사용하는 백엔드만 선택해 활성화(Codex만 / 요약 백엔드만 / 둘 다).

---

## 왜 만들었는가

**Codex** 같은 AI 코딩 에이전트는 대화 내용을 JSONL/JSON으로 홈 디렉토리 밑에 조용히 저장한다. 매일 쓰다 보면 몇 달 만에 파일이 불어나는데, 제 경우 `~/.codex/sessions/`는 **3 GB**를 넘겼다.

이 기록은 분명 귀중하다. 몇 달 치의 디버깅 노트, 설계 결정, 공들여 다듬은 프롬프트가 고스란히 들어있다. 하지만 3 GB짜리 파일은 디스크에 둔 채로는 너무 무겁고, raw 그대로 뒤지기엔 고통스럽고, 그렇다고 지우기엔 아깝다. 결국 자리만 차지하고 아무 쓸모가 없는 **죽은 무게**가 되어 있었다.

**Session Butler는 이 딜레마를 푼다.** 오래된 세션의 용량을 줄이는 동시에, 검색하고 다시 꺼내 쓸 수 있는 개인 지식베이스로 바꿔서 폴더 속에서 썩히지 않게 만든다.

## 어떤 일을 하나

이 도구는 **Codex** 세션 로그를 관리하고, `session_*.json` 로그(**요약 백엔드**)를 요약합니다. 각 명령은 한쪽 백엔드를 대상으로 하며, 설정에서 백엔드별 활성화를 제어합니다(아래 설정 참고).

### Codex — 세션 로그 관리

| 명령 | 하는 일 |
|------|---------|
| `scan` | Codex `rollout-*.jsonl` **전체**를 순회하며 메타데이터 + FTS5 전문검색 인덱스를 SQLite에 저장 |
| `archive` | 보존 기간보다 **오래된** 세션을 zstd 압축(최근 N일은 보존). `--move`는 압축 후 원본 삭제, `--skip-scan`은 사전 스캔 생략 |
| `restore` | `.zst`에서 복원 — **DB 아카이브 인덱스**를 읽습니다(원본이 없어도 동작). `--purge`는 복원 후 `.zst` 삭제 |
| `list` / `stats` | 최근 N일 세션의 목록 및 통계 |
| `insights` | 인덱싱된 데이터로 사용 인사이트 — tool/skill, 프로젝트, 토큰/시간 추세(`--by day/week/month`), 상위 단어 |
| `compact` | 안전한 컴팩션 + 민감정보 탐지(`.env`, token, key) |

아카이브 상태와 SHA-256 체크섬은 **SQLite 인덱스**에 저장되어, `restore`가 무결성을 검증하고 원본 삭제 후에도 세션을 찾을 수 있습니다.

> **`--days`의 범위:** `scan`은 항상 **모든** 세션을 색인합니다(`--days` 없음). `--days`는 다른 명령의 대상만 좁힙니다 — `archive`/`compact`/`pipeline`에서는 **보존 기간**(최근 N일은 두고 그 이전에 작업), `list`/`stats`/`insights`에서는 **조회 기간**(최근 N일, `insights`는 `0` = 전체).

### 요약 백엔드 — 세션 로그 요약

| 명령 | 하는 일 |
|------|---------|
| `summarize` | `session_*.json` 분석 → 요약 + FTS5 키워드 JSON |

참고: 이 백엔드는 여러 종류의 파일(`session_*.json`, `request_dump_*.json` 등)을 기록합니다. 실제 대화 로그인 `session_*.json`만 요약하며, 요청/에러 덤프는 건너뜁니다.

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

TUI는 상단에 **status 바**로 활성 백엔드, 세션 경로, 보존 일수를 항상 표시하며, 활성화된 백엔드의 명령만 목록에 보여줍니다.

명령 실행 중에는 TUI가 잠시 터미널을 내주어 **실시간 진행률 바**(스피너 + 바 + `N/전체` + `%` + ETA)가 보이고, 끝나면 캡처한 출력을 담은 **Results** 패널로 돌아옵니다.

### CLI

```bash
# Codex — 스캔 + 인덱싱
session-butler scan [--analyze]

# Codex — 압축 / 복원
session-butler archive  --days 30 --dry-run    # 미리보기
session-butler archive  --days 30              # 압축 (원본 보존)
session-butler archive  --days 30 --move       # 압축 후 원본 삭제
session-butler restore  --all                  # 복원 (.zst 유지, 재복원 가능)
session-butler restore  --all --purge          # 복원 후 .zst 삭제

# Codex — 목록 / 통계 / 인사이트 / 컴팩션
session-butler list   --days 30 [--json]
session-butler stats  --days 30
session-butler insights [--days 0] [--top 15] [--by month]   # 0 = 전체
session-butler compact --scan-sensitive        # 민감정보 스캔만

# 요약
session-butler summarize                       # 요약 + FTS5 JSON
session-butler summarize --summary-only
session-butler summarize --fts-only

# 한 번에 순차 실행
session-butler pipeline --days 30 --dry-run
```

## 설정

### 백엔드 활성화

Codex와 요약 백엔드 각각 활성화/비활성화할 수 있습니다. 우선순위(높은 것이 이김):

1. CLI 플래그: `--no-codex`, `--no-hermes`
2. 환경변수: `CODEX_ENABLED`, `HERMES_ENABLED`(`0`/`false`/`off`/`no` → 비활성)
3. 설정 파일: `~/.config/session-butler/config.json`
4. 기본값: 둘 다 활성

비활성 백엔드의 명령은 no-op가 되며, `pipeline`은 해당 백엔드 작업을 자동으로 건너뜁니다.

설정 파일 예시:

```json
{
  "enabled_codex": true,
  "enabled_hermes": false
}
```

### 경로 및 출력 (공용 옵션 또는 환경변수)

| 플래그 | 환경변수 | 기본값 |
|------|---------|---------|
| `-C, --codex-sessions <DIR>` | `CODEX_SESSIONS` | `~/.codex/sessions` |
| `-H, --hermes-sessions <DIR>` | `HERMES_SESSIONS` | `~/.hermes/sessions` |
| `-A, --codex-archive <DIR>` | `CODEX_ARCHIVE` | `~/.codex/archive` |
| `-I, --codex-index-db <PATH>` | `CODEX_INDEX_DB` | `./codex_index.sqlite` |
| `-S, --summary-layer <PATH>` | `SUMMARY_LAYER_JSON` | `./summary_layer.json` |
| `-F, --fts5-index <PATH>` | `FTS5_INDEX_JSON` | `./fts5_index.json` |

## 결과

실제 제 세션 기록으로 측정한 수치:

| 대상 | 파일 수 | 원본 크기 | 결과 |
|------|--------:|---------:|-------|
| Codex 세션 | 3,037 | 3.1 GB | 압축 대상(2,303개) **2.42 GB → 0.86 GB** (약 64% 축소) |
| 요약 백엔드 세션 | 82 (그중 `session_*.json` 52개) | 47 MB | 52개 세션 요약 → `summary_layer.json` + `fts5_index.json` |

즉, **2.4 GB짜리 과거 Codex 세션이 약 860 MB로 줄었고**, SQLite + FTS5 인덱스로 전문검색도 그대로 가능하다. `archive --move`로 원본을 지우면 그만큼 디스크를 추가로 확보할 수 있다.

## 안전성

- 기본적으로 어떤 명령도 원본을 삭제하지 않습니다. 압축은 `.jsonl.zst` 사본을 만들 뿐, 원본은 그대로 둡니다.
- 명시적 삭제만 가능: `archive --move`(압축 후 원본 삭제), `restore --purge`(복원 후 `.zst` 삭제).
- 보존 기간(`--days`, 기본 `30`)보다 최근 세션은 대상에서 제외된다.
- 압축 파일마다 SHA-256 체크섬을 SQLite 인덱스에 저장하므로, `restore`가 무결성을 검증하고 손상을 감지합니다.

## 개발 현황

스캔/인덱싱, 압축/복원(SQLite 기반), 컴팩션, 사용 인사이트, 요약, TUI(실시간 진행률 포함), CLI 모두 동작한다.

## 라이선스

[MIT](../LICENSE) © Kim Jeongjin
