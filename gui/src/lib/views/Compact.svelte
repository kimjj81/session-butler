<script lang="ts">
  import { runCompact, runScanSensitive, fmtInt, type ScanProgress, type CompactSummary, type SensitiveFile } from "$lib/api";

  // ---- Compact 상태 ----
  let days = $state(30);
  let dryRun = $state(true);

  let compactRunning = $state(false);
  let compactError = $state<string | null>(null);
  let compactResult = $state<CompactSummary | null>(null);

  let compactPos = $state(0);
  let compactLen = $state(0);
  let compactMsg = $state("");

  function compactPct(): number {
    if (compactLen <= 0) return 0;
    return Math.min(100, Math.round((compactPos / compactLen) * 100));
  }

  function onCompactProgress(p: ScanProgress) {
    if (p.kind === "bar") {
      compactPos = 0;
      compactLen = p.len ?? 0;
      compactMsg = p.msg ?? "";
    } else if (p.kind === "spinner") {
      compactMsg = p.msg ?? "";
    } else if (p.kind === "inc") {
      compactPos += p.n ?? 0;
    } else if (p.kind === "finish") {
      compactMsg = p.msg ?? "";
    }
  }

  async function doCompact() {
    if (compactRunning) return;
    compactRunning = true;
    compactError = null;
    compactResult = null;
    compactPos = 0;
    compactLen = 0;
    compactMsg = "Compact 준비 중…";
    try {
      compactResult = await runCompact(days, dryRun, onCompactProgress);
    } catch (e: any) {
      compactError = String(e);
      compactResult = null;
    } finally {
      compactRunning = false;
    }
  }

  // ---- 민감정보 스캔 상태 ----
  let scanRunning = $state(false);
  let scanError = $state<string | null>(null);
  let scanResult = $state<SensitiveFile[] | null>(null);

  let scanPos = $state(0);
  let scanLen = $state(0);
  let scanMsg = $state("");

  function scanPct(): number {
    if (scanLen <= 0) return 0;
    return Math.min(100, Math.round((scanPos / scanLen) * 100));
  }

  function onScanProgress(p: ScanProgress) {
    if (p.kind === "bar") {
      scanPos = 0;
      scanLen = p.len ?? 0;
      scanMsg = p.msg ?? "";
    } else if (p.kind === "spinner") {
      scanMsg = p.msg ?? "";
    } else if (p.kind === "inc") {
      scanPos += p.n ?? 0;
    } else if (p.kind === "finish") {
      scanMsg = p.msg ?? "";
    }
  }

  async function doScanSensitive() {
    if (scanRunning) return;
    scanRunning = true;
    scanError = null;
    scanResult = null;
    scanPos = 0;
    scanLen = 0;
    scanMsg = "스캔 준비 중…";
    try {
      scanResult = await runScanSensitive(onScanProgress);
    } catch (e: any) {
      scanError = String(e);
      scanResult = null;
    } finally {
      scanRunning = false;
    }
  }
</script>

<section>
  <h2>Compact</h2>

  <div class="controls">
    <label>보존일수 (days)
      <input type="number" min="0" bind:value={days} disabled={compactRunning} />
    </label>
    <label class="check">
      <input type="checkbox" bind:checked={dryRun} disabled={compactRunning} />
      dry-run (미리보기)
    </label>
    <button class="run" onclick={doCompact} disabled={compactRunning}>
      {compactRunning ? "실행 중…" : "Compact 실행"}
    </button>
  </div>

  {#if dryRun && !compactRunning}
    <div class="note">dry-run 모드 — 실제로 이동하지 않고 대상만 집계합니다.</div>
  {/if}

  {#if compactRunning}
    <div class="progress">
      <div class="progress-msg">{compactMsg} — {fmtInt(compactPos)}/{fmtInt(compactLen)} ({compactPct()}%)</div>
      <div class="bar"><div class="fill" style="width:{compactPct()}%"></div></div>
    </div>
  {/if}

  {#if compactError}
    <div class="error">오류: {compactError}</div>
  {/if}

  {#if compactResult}
    <div class="result">
      <div class="result-row"><span class="rk">이동(moved)</span><span class="rv">{fmtInt(compactResult.moved)}</span></div>
      <div class="result-row"><span class="rk">건너뜀(skipped)</span><span class="rv">{fmtInt(compactResult.skipped)}</span></div>
      <div class="result-row"><span class="rk">전체(total)</span><span class="rv">{fmtInt(compactResult.total)}</span></div>
    </div>
  {/if}
</section>

<hr class="divider" />

<section>
  <h2>민감정보 스캔</h2>

  <div class="controls">
    <button class="run" onclick={doScanSensitive} disabled={scanRunning}>
      {scanRunning ? "스캔 중…" : "민감정보 스캔"}
    </button>
  </div>

  {#if scanRunning}
    <div class="progress">
      <div class="progress-msg">{scanMsg} — {fmtInt(scanPos)}/{fmtInt(scanLen)} ({scanPct()}%)</div>
      <div class="bar"><div class="fill" style="width:{scanPct()}%"></div></div>
    </div>
  {/if}

  {#if scanError}
    <div class="error">오류: {scanError}</div>
  {/if}

  {#if scanResult}
    {#if scanResult.length === 0}
      <p class="muted">없음</p>
    {:else}
      <table>
        <thead>
          <tr>
            <th>경로</th>
            <th>날짜</th>
            <th class="r">크기(bytes)</th>
            <th>패턴</th>
          </tr>
        </thead>
        <tbody>
          {#each scanResult as f}
            <tr>
              <td class="mono" title={f.path}>{f.path}</td>
              <td>{f.date ?? "-"}</td>
              <td class="r">{fmtInt(f.size_bytes)}</td>
              <td class="patterns">{f.patterns.length ? f.patterns.join(", ") : "-"}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  {/if}
</section>

<style>
  section { background: #1b2026; border: 1px solid #262d34; border-radius: 8px; padding: 14px 16px; margin-bottom: 16px; }
  h2 { font-size: 14px; margin: 0 0 10px; color: #c9ced3; }

  .controls { display: flex; flex-wrap: wrap; gap: 10px; align-items: end; }
  .controls label { display: flex; flex-direction: column; font-size: 12px; color: #9aa1a8; gap: 4px; }
  .controls label.check { flex-direction: row; align-items: center; gap: 6px; font-size: 13px; color: #e6e8eb; }
  input[type="number"] {
    background: #1f242b; color: #e6e8eb; border: 1px solid #2c333b; border-radius: 6px;
    padding: 6px 8px; font-size: 13px; width: 92px;
  }
  input[type="checkbox"] { width: auto; accent-color: #2f6fed; }

  button { background: #2a3138; color: #e6e8eb; border: 1px solid #3a434c; border-radius: 6px; padding: 7px 12px; cursor: pointer; font-size: 13px; }
  button:disabled { opacity: 0.5; cursor: default; }
  button.run { background: #1f7a4d; border-color: #2a9c62; }
  button.run:disabled { background: #1f7a4d; border-color: #2a9c62; }

  .note { background: #2a2a1f; border: 1px solid #5a5a2a; color: #9aa1a8; padding: 8px 12px; border-radius: 6px; margin: 12px 0 0; font-size: 13px; }

  .progress { margin-top: 14px; }
  .progress-msg { font-size: 12px; color: #9aa1a8; margin-bottom: 4px; }
  .bar { height: 8px; background: #1f242b; border-radius: 4px; overflow: hidden; }
  .fill { height: 100%; background: #2a9c62; transition: width 0.15s; }

  .error { background: #3a1f1f; border: 1px solid #6b2a2a; color: #ffb4b4; padding: 10px 12px; border-radius: 6px; margin-top: 14px; font-size: 13px; }

  .result { margin-top: 14px; display: flex; flex-wrap: wrap; gap: 10px; }
  .result-row { background: #14181c; border: 1px solid #2c333b; border-radius: 6px; padding: 8px 12px; display: flex; flex-direction: column; min-width: 120px; }
  .result-row .rk { font-size: 11px; color: #9aa1a8; }
  .result-row .rv { font-size: 18px; font-weight: 600; margin-top: 2px; font-variant-numeric: tabular-nums; }

  hr.divider { border: none; border-top: 1px solid #262d34; margin: 4px 0 16px; }

  table { width: 100%; border-collapse: collapse; font-size: 13px; margin-top: 14px; }
  th { text-align: left; color: #9aa1a8; font-weight: 500; padding: 4px 6px; border-bottom: 1px solid #2c333b; }
  td { padding: 4px 6px; border-bottom: 1px solid #22282e; vertical-align: top; }
  th.r, td.r { text-align: right; }
  .mono { font-family: ui-monospace, monospace; }
  td.patterns { color: #c9ced3; max-width: 320px; word-break: break-word; }

  .muted { color: #9aa1a8; font-size: 14px; margin-top: 14px; }
</style>
