<script lang="ts">
  import { runSummarize } from "$lib/api";

  let running = $state(false);
  let error = $state<string | null>(null);
  let done = $state(false);
  let lastLabel = $state("");

  async function run(summaryOnly: boolean, ftsOnly: boolean, label: string) {
    if (running) return;
    running = true;
    error = null;
    done = false;
    lastLabel = label;
    try {
      await runSummarize(summaryOnly, ftsOnly);
      done = true;
    } catch (e: any) {
      error = String(e);
      done = false;
    } finally {
      running = false;
    }
  }

  function runBoth() { run(false, false, "요약 + FTS5 생성"); }
  function runSummaryOnly() { run(true, false, "요약만"); }
  function runFtsOnly() { run(false, true, "FTS5만"); }
</script>

<section>
  <h2>Summarize</h2>

  <p class="desc">Hermes session_*.json → 요약 + FTS5 인덱스 생성</p>

  <div class="note">
    summary 백엔드 비활성 가능성 — 설정 확인
  </div>

  <div class="buttons">
    <button class="run" onclick={runBoth} disabled={running}>
      {running && lastLabel === "요약 + FTS5 생성" ? "생성 중…" : "요약 + FTS5 생성"}
    </button>
    <button class="run" onclick={runSummaryOnly} disabled={running}>
      {running && lastLabel === "요약만" ? "생성 중…" : "요약만"}
    </button>
    <button class="run" onclick={runFtsOnly} disabled={running}>
      {running && lastLabel === "FTS5만" ? "생성 중…" : "FTS5만"}
    </button>
  </div>

  {#if running}
    <div class="status">생성 중… ({lastLabel})</div>
  {/if}

  {#if done && !running}
    <div class="success">완료</div>
  {/if}

  {#if error}
    <div class="error">오류: {error}</div>
  {/if}
</section>

<style>
  section { background: #1b2026; border: 1px solid #262d34; border-radius: 8px; padding: 14px 16px; margin-bottom: 16px; }
  h2 { font-size: 14px; margin: 0 0 8px; color: #c9ced3; }

  .desc { font-size: 13px; color: #9aa1a8; margin: 0 0 12px; }

  .note { background: #2a2a1f; border: 1px solid #5a5a2a; color: #c9aa6b; padding: 8px 12px; border-radius: 6px; margin: 0 0 14px; font-size: 12px; }

  .buttons { display: flex; flex-wrap: wrap; gap: 10px; }
  button {
    background: #1f7a4d; color: #e6e8eb; border: 1px solid #2a9c62; border-radius: 6px;
    padding: 7px 14px; cursor: pointer; font-size: 13px;
  }
  button:disabled { opacity: 0.5; cursor: default; }

  .status { margin-top: 14px; font-size: 13px; color: #9aa1a8; }

  .success { background: #1f2a20; border: 1px solid #2a6b3a; color: #9bd1ad; padding: 10px 12px; border-radius: 6px; margin-top: 14px; font-size: 13px; }

  .error { background: #3a1f1f; border: 1px solid #6b2a2a; color: #ffb4b4; padding: 10px 12px; border-radius: 6px; margin-top: 14px; font-size: 13px; }
</style>
