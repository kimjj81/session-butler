<script lang="ts">
  import { onMount } from "svelte";
  import {
    listArchived, runRestore, fmtInt,
    type ArchivedRow, type RestoreSummary, type ScanProgress,
  } from "$lib/api";

  let rows = $state<ArchivedRow[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);

  // 진행률
  let running = $state(false);
  let pos = $state(0);
  let len = $state(0);
  let msg = $state("");

  // 결과
  let result = $state<RestoreSummary | null>(null);

  let pct = $derived(len > 0 ? Math.min(100, Math.round((pos / len) * 100)) : 0);

  async function load() {
    loading = true;
    error = null;
    try {
      rows = await listArchived();
    } catch (e: any) {
      error = String(e);
      rows = [];
    } finally {
      loading = false;
    }
  }

  function onProgress(p: ScanProgress) {
    if (p.kind === "bar") {
      msg = p.msg ?? "";
      pos = 0;
      len = p.len ?? 0;
    } else if (p.kind === "spinner") {
      msg = p.msg ?? "";
    } else if (p.kind === "inc") {
      pos += p.n ?? 0;
    }
  }

  async function doRestore(dryRun: boolean, purge: boolean) {
    if (running) return;
    running = true;
    pos = 0;
    len = 0;
    msg = dryRun ? "복원(dry-run) 준비 중…" : "복원 준비 중…";
    result = null;
    error = null;
    try {
      result = await runRestore(dryRun, purge, onProgress);
      await load();
    } catch (e: any) {
      error = String(e);
    } finally {
      running = false;
    }
  }

  function short(s: string, n: number): string {
    return s.length > n ? s.slice(0, n) + "…" : s;
  }

  onMount(load);
</script>

<section>
  <div class="head">
    <h2>Restore</h2>
    <span class="count">보관 {fmtInt(rows.length)}건</span>
  </div>

  <div class="actions">
    <button class="run" onclick={() => doRestore(true, false)} disabled={running || loading}>
      전체 복원(dry-run)
    </button>
    <button class="purge" onclick={() => doRestore(false, true)} disabled={running || loading}>
      전체 복원 + .zst 삭제
    </button>
  </div>

  {#if running}
    <div class="scan-bar">
      <div class="scan-msg">{msg}{#if len > 0} — {fmtInt(pos)}/{fmtInt(len)} ({pct}%){:else} — 진행 중…{/if}</div>
      <div class="bar"><div class="fill" class:indet={len === 0} style="width:{len > 0 ? pct : 100}%"></div></div>
    </div>
  {/if}

  {#if result}
    <div class="result">복원 완료: {fmtInt(result.restored)}건</div>
  {/if}

  {#if error}
    <div class="error">오류: {error}</div>
  {/if}

  {#if loading}
    <p class="muted">로딩 중…</p>
  {:else if rows.length === 0 && !error}
    <p class="empty">보관된 세션 없음</p>
  {:else}
    <table>
      <thead>
        <tr>
          <th>session_id</th>
          <th>date</th>
          <th>path</th>
        </tr>
      </thead>
      <tbody>
        {#each rows as r}
          <tr>
            <td class="mono" title={r.session_id}>{short(r.session_id, 26)}</td>
            <td>{r.date ?? "-"}</td>
            <td class="mono" title={r.path}>{short(r.path, 48)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</section>

<style>
  section {
    background: #1b2026;
    border: 1px solid #262d34;
    border-radius: 8px;
    padding: 14px 16px;
    margin-bottom: 16px;
  }
  .head { display: flex; align-items: baseline; gap: 12px; margin-bottom: 12px; }
  h2 { font-size: 14px; margin: 0; color: #c9ced3; }
  .count { font-size: 12px; color: #9aa1a8; }

  .actions { display: flex; gap: 8px; margin-bottom: 12px; }
  button {
    background: #2a3138;
    color: #e6e8eb;
    border: 1px solid #3a434c;
    border-radius: 6px;
    padding: 7px 12px;
    cursor: pointer;
    font-size: 13px;
  }
  button:disabled { opacity: 0.5; cursor: default; }
  button.run { background: #1f7a4d; border-color: #2a9c62; }
  button.purge { background: #2f6fed; border-color: #4a82f5; }

  .scan-bar { margin-bottom: 12px; }
  .scan-msg { font-size: 12px; color: #9aa1a8; margin-bottom: 4px; }
  .bar { height: 8px; background: #1f242b; border-radius: 4px; overflow: hidden; }
  .fill { height: 100%; background: #2a9c62; transition: width 0.15s; }
  .fill.indet { animation: indet 1.1s ease-in-out infinite; }
  @keyframes indet { 0% { opacity: 0.3; } 50% { opacity: 1; } 100% { opacity: 0.3; } }

  .result {
    background: #1f2a1f;
    border: 1px solid #2a5a2a;
    color: #b4ffb4;
    padding: 8px 12px;
    border-radius: 6px;
    margin-bottom: 12px;
    font-size: 13px;
  }
  .error {
    background: #3a1f1f;
    border: 1px solid #6b2a2a;
    color: #ffb4b4;
    padding: 10px 12px;
    border-radius: 6px;
    margin-bottom: 12px;
    font-size: 13px;
  }
  .empty, .muted { color: #9aa1a8; font-size: 14px; }

  table { width: 100%; border-collapse: collapse; font-size: 13px; }
  th { text-align: left; color: #9aa1a8; font-weight: 500; padding: 4px 6px; border-bottom: 1px solid #2c333b; }
  td { padding: 4px 6px; border-bottom: 1px solid #22282e; }
  .mono { font-family: ui-monospace, monospace; }
</style>
