<script lang="ts">
  import { runArchive, fmtInt, type ScanProgress, type ArchiveSummary } from "$lib/api";

  let days = $state(30);
  let dryRun = $state(true);
  let moveOriginals = $state(false);

  let running = $state(false);
  let error = $state<string | null>(null);
  let result = $state<ArchiveSummary | null>(null);

  // 진행률
  let pos = $state(0);
  let len = $state(0);
  let msg = $state("");

  let pct = $derived(len > 0 ? Math.min(100, Math.round((pos / len) * 100)) : 0);

  function onProgress(p: ScanProgress) {
    if (p.kind === "bar") {
      pos = 0;
      len = p.len ?? 0;
      msg = p.msg ?? "";
    } else if (p.kind === "spinner") {
      msg = p.msg ?? "";
    } else if (p.kind === "inc") {
      pos += p.n ?? 0;
    } else if (p.kind === "finish") {
      msg = p.msg ?? msg;
    }
  }

  let ratio = $derived(
    result && result.total_original > 0
      ? Math.round((1 - result.total_compressed / result.total_original) * 100)
      : null,
  );

  async function run() {
    if (running) return;
    running = true;
    error = null;
    result = null;
    pos = 0;
    len = 0;
    msg = "아카이브 준비 중…";
    try {
      result = await runArchive(days, dryRun, moveOriginals, onProgress);
    } catch (e: any) {
      error = String(e);
      result = null;
    } finally {
      running = false;
    }
  }
</script>

<section>
  <h2>Archive</h2>

  <div class="form">
    <label class="field">
      <span>보존일수 (days)</span>
      <input type="number" min="0" bind:value={days} disabled={running} />
    </label>

    <label class="check">
      <input type="checkbox" bind:checked={dryRun} disabled={running} />
      <span>dry-run (미리보기)</span>
    </label>

    <label class="check">
      <input type="checkbox" bind:checked={moveOriginals} disabled={running} />
      <span>압축 후 원본 삭제</span>
    </label>

    <button class="run" onclick={run} disabled={running}>
      {running ? "실행 중…" : "Archive 실행"}
    </button>
  </div>

  {#if running}
    <div class="prog">
      <div class="prog-msg">
        {msg}{#if len > 0} — {fmtInt(pos)}/{fmtInt(len)} ({pct}%){:else} — 진행 중…{/if}
      </div>
      <div class="bar"><div class="fill" class:indet={len === 0} style="width:{len > 0 ? pct : 100}%"></div></div>
    </div>
  {/if}

  {#if error}
    <div class="error">오류: {error}</div>
  {/if}

  {#if result}
    <div class="result">
      {#if dryRun}<span class="badge-dry">미리보기 (dry-run)</span>{/if}

      <div class="cards">
        <div class="card">
          <div class="k">archived</div>
          <div class="v">{fmtInt(result.archived)}</div>
        </div>
        <div class="card">
          <div class="k">skipped</div>
          <div class="v">{fmtInt(result.skipped)}</div>
        </div>
        <div class="card">
          <div class="k">압축률</div>
          <div class="v">{ratio !== null ? `${ratio}%` : "-"}</div>
        </div>
      </div>

      <div class="bytes">
        <span class="mono">{fmtInt(result.total_original)} B</span>
        <span class="arrow">→</span>
        <span class="mono">{fmtInt(result.total_compressed)} B</span>
      </div>
    </div>
  {/if}
</section>

<style>
  section { background: #1b2026; border: 1px solid #262d34; border-radius: 8px; padding: 14px 16px; margin-bottom: 16px; }
  h2 { font-size: 14px; margin: 0 0 12px; color: #c9ced3; }

  .form { display: flex; flex-wrap: wrap; gap: 12px; align-items: end; }
  .field { display: flex; flex-direction: column; font-size: 12px; color: #9aa1a8; gap: 4px; }
  .check { display: flex; align-items: center; gap: 6px; font-size: 13px; color: #e6e8eb; cursor: pointer; }
  input[type="number"] {
    background: #1f242b; color: #e6e8eb; border: 1px solid #2c333b; border-radius: 6px;
    padding: 6px 8px; font-size: 13px; width: 110px;
  }
  input[type="checkbox"] { accent-color: #2f6fed; }
  button.run { background: #1f7a4d; color: #e6e8eb; border: 1px solid #2a9c62; border-radius: 6px; padding: 7px 14px; cursor: pointer; font-size: 13px; }
  button.run:disabled { opacity: 0.5; cursor: default; }

  .prog { margin-top: 14px; }
  .prog-msg { font-size: 12px; color: #9aa1a8; margin-bottom: 4px; }
  .bar { height: 8px; background: #1f242b; border-radius: 4px; overflow: hidden; }
  .fill { height: 100%; background: #2a9c62; transition: width 0.15s; }
  .fill.indet { animation: indet 1.1s ease-in-out infinite; }
  @keyframes indet { 0% { opacity: 0.3; } 50% { opacity: 1; } 100% { opacity: 0.3; } }

  .error { background: #3a1f1f; border: 1px solid #6b2a2a; color: #ffb4b4; padding: 10px 12px; border-radius: 6px; margin-top: 14px; font-size: 13px; }

  .result { margin-top: 16px; }
  .badge-dry { display: inline-block; font-size: 11px; color: #c9aa6b; background: #2a261f; border: 1px solid #5a4a2a; padding: 3px 8px; border-radius: 6px; margin-bottom: 10px; }

  .cards { display: grid; grid-template-columns: repeat(auto-fill, minmax(110px, 1fr)); gap: 10px; }
  .card { background: #14161a; border: 1px solid #262d34; border-radius: 8px; padding: 12px; }
  .card .k { font-size: 11px; color: #9aa1a8; }
  .card .v { font-size: 20px; font-weight: 600; margin-top: 4px; }

  .bytes { margin-top: 12px; font-size: 13px; color: #9aa1a8; display: flex; align-items: center; gap: 8px; }
  .bytes .arrow { color: #6b7178; }
  .mono { font-family: ui-monospace, monospace; color: #e6e8eb; font-variant-numeric: tabular-nums; }
</style>
