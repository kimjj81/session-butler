<script lang="ts">
  import { onMount } from "svelte";
  import {
    getInsights, runScan, fmtInt,
    type Report, type ScanProgress,
  } from "$lib/api";
  import Archive from "$lib/views/Archive.svelte";
  import Chart from "$lib/Chart.svelte";
  import Restore from "$lib/views/Restore.svelte";
  import Compact from "$lib/views/Compact.svelte";
  import Summarize from "$lib/views/Summarize.svelte";
  import Settings from "$lib/views/Settings.svelte";

  let tab = $state<"dashboard" | "archive" | "restore" | "compact" | "summarize" | "settings">("dashboard");

  let report = $state<Report | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);

  // 컨트롤
  let days = $state(0);
  let top = $state(15);
  let by = $state("month");
  let words = $state("all");

  // 스캔 상태
  let scanning = $state(false);
  let scanPos = $state(0);
  let scanLen = $state(0);
  let scanMsg = $state("");
  let scanWarns = $state(0);

  const WORDS_LABEL: Record<string, string> = {
    conversation: "대화", reasoning: "추론", tools: "도구·출력", "first-prompt": "첫 프롬프트", all: "전체(카테고리별)",
  };

  function scanPct(): number {
    if (scanLen <= 0) return 0;
    return Math.min(100, Math.round((scanPos / scanLen) * 100));
  }

  async function load() {
    loading = true;
    error = null;
    try {
      report = await getInsights(days, top, by, words);
    } catch (e: any) {
      error = String(e);
      report = null;
    } finally {
      loading = false;
    }
  }

  function onScanProgress(p: ScanProgress) {
    if (p.kind === "bar" || p.kind === "spinner") {
      scanMsg = p.msg ?? "";
      if (p.kind === "bar") { scanPos = 0; scanLen = p.len ?? 0; }
    } else if (p.kind === "inc") {
      scanPos += p.n ?? 0;
    } else if (p.kind === "warn") {
      scanWarns += 1;
    }
  }

  async function doScan() {
    if (scanning) return;
    scanning = true; scanPos = 0; scanLen = 0; scanWarns = 0; scanMsg = "스캔 준비 중…";
    try {
      await runScan(onScanProgress);
      await load();
    } catch (e: any) {
      error = String(e);
    } finally {
      scanning = false;
    }
  }

  // ---- Chart.js 데이터 ----
  const BAR = "#2f6fed";
  const xGrid = { display: false } as const;
  const yGrid = { color: "#22282e" } as const;
  const noLegend = { legend: { display: false } } as const;
  const horizontalBarOpts = { indexAxis: "y" as const, plugins: noLegend, scales: { x: { grid: yGrid }, y: { grid: xGrid } } };
  const barOpts = { plugins: noLegend, scales: { x: { grid: xGrid }, y: { grid: yGrid } } };
  const lineOpts = { plugins: noLegend, scales: { x: { grid: xGrid }, y: { grid: yGrid } } };

  // $derived: report 가 바뀔 때만 재계산 → Chart 가 매 flush 마다 destroy/재생성하지 않음.
  let toolsData = $derived(
    (() => {
      const t = report?.top_tools ?? [];
      return { labels: t.map((x) => x.tool), datasets: [{ data: t.map((x) => x.calls), backgroundColor: BAR, borderRadius: 4 }] };
    })(),
  );
  let weekdayData = $derived(
    (() => {
      const w = report?.activity_by_weekday ?? [];
      return { labels: w.map((x) => x.weekday), datasets: [{ data: w.map((x) => x.sessions), backgroundColor: BAR, borderRadius: 4 }] };
    })(),
  );
  let trendData = $derived(
    (() => {
      // 버킷은 내림차순(최근 위) → 시간순 좌→우 로 reverse
      const b = [...(report?.time_buckets ?? [])].reverse();
      return {
        labels: b.map((x) => x.label),
        datasets: [{ label: "세션", data: b.map((x) => x.sessions), borderColor: BAR, backgroundColor: "rgba(47,111,237,0.15)", tension: 0.3, fill: true, pointRadius: 2 }],
      };
    })(),
  );

  onMount(load);
</script>

<main>
  <header>
    <h1>Session Butler</h1>
    <div class="controls">
      <label>기간(일, 0=전체)
        <input type="number" min="0" bind:value={days} />
      </label>
      <label>Top-N
        <input type="number" min="1" bind:value={top} />
      </label>
      <label>버킷
        <select bind:value={by}>
          <option value="day">day</option>
          <option value="week">week</option>
          <option value="month">month</option>
        </select>
      </label>
      <label>단어 소스
        <select bind:value={words}>
          <option value="all">all</option>
          <option value="conversation">conversation</option>
          <option value="reasoning">reasoning</option>
          <option value="tools">tools</option>
          <option value="first-prompt">first-prompt</option>
        </select>
      </label>
      <button class="primary" onclick={load} disabled={loading || scanning}>
        {loading ? "로딩…" : "새로고침"}
      </button>
      <button class="scan" onclick={doScan} disabled={scanning}>
        {scanning ? "스캔 중…" : "Scan 실행"}
      </button>
    </div>

    {#if scanning}
      <div class="scan-bar">
        <div class="scan-msg">
          {scanMsg} — {fmtInt(scanPos)}/{fmtInt(scanLen)} ({scanPct()}%)
          {#if scanWarns > 0}<span class="warn"> · 실패 {fmtInt(scanWarns)}건</span>{/if}
        </div>
        <div class="bar"><div class="fill" style="width:{scanPct()}%"></div></div>
      </div>
    {/if}
  </header>

  <nav class="tabs">
    {#each [["dashboard", "대시보드"], ["archive", "Archive"], ["restore", "Restore"], ["compact", "Compact"], ["summarize", "Summarize"], ["settings", "Settings"]] as [id, label]}
      <button class:active={tab === id} onclick={() => (tab = id as typeof tab)}>{label}</button>
    {/each}
  </nav>

  {#if tab === "dashboard"}
  {#if error}
    <div class="error">오류: {error}</div>
  {/if}

  {#if !report && !loading}
    <div class="empty">데이터가 없습니다. <strong>Scan 실행</strong>으로 인덱스를 만드세요.</div>
  {:else if report}
    {#if report.words_fallback}
      <div class="note">
        단어 카테고리 데이터가 없어 첫 프롬프트 기반으로 표시합니다. Scan 재실행 시 대화/추론/도구별 분석이 활성화됩니다.
      </div>
    {/if}

    <section class="cards">
      <div class="card"><div class="k">세션</div><div class="v">{fmtInt(report.overview.sessions)}</div></div>
      <div class="card"><div class="k">총 토큰</div><div class="v">{fmtInt(report.overview.total_tokens)}</div></div>
      <div class="card"><div class="k">툴 호출</div><div class="v">{fmtInt(report.overview.total_tool_calls)}</div></div>
      <div class="card"><div class="k">파일 변경</div><div class="v">{fmtInt(report.overview.total_file_changes)}</div></div>
      <div class="card"><div class="k">프로젝트</div><div class="v">{fmtInt(report.overview.distinct_projects)}</div></div>
      <div class="card"><div class="k">tool 종류</div><div class="v">{fmtInt(report.overview.distinct_tools)}</div></div>
      <div class="card"><div class="k">보관(archived)</div><div class="v">{fmtInt(report.overview.archived)}</div></div>
      <div class="card"><div class="k">피크 시간</div><div class="v">{report.peak_hour != null ? `${report.peak_hour}:00` : "-"}</div></div>
    </section>
    {#if report.overview.date_from && report.overview.date_to}
      <div class="range">기간: {report.overview.date_from} ~ {report.overview.date_to}</div>
    {/if}

    <div class="grid">
      <section>
        <h2>자주 쓴 tool/skill (top {report.top_tools.length})</h2>
        {#if report.top_tools.length === 0}<p class="muted">없음</p>{:else}
        <Chart type="bar" data={toolsData} options={horizontalBarOpts} height={Math.min(360, report.top_tools.length * 24 + 30)} />
        <details class="exact">
          <summary>정확한 수치 ({report.top_tools.length})</summary>
          <ul class="kv">
            {#each report.top_tools as t}<li><span class="mono" title={t.tool}>{t.tool}</span><span class="num">{fmtInt(t.calls)}</span></li>{/each}
          </ul>
        </details>
        {/if}
      </section>

      <section>
        <h2>프로젝트별</h2>
        {#if report.top_projects.length === 0}<p class="muted">없음</p>{:else}
        <table>
          <thead><tr><th>repo</th><th class="r">세션</th><th class="r">토큰</th></tr></thead>
          <tbody>
            {#each report.top_projects as p}
              <tr><td>{p.repo}</td><td class="r">{fmtInt(p.sessions)}</td><td class="r">{fmtInt(p.tokens)}</td></tr>
            {/each}
          </tbody>
        </table>
        {/if}
      </section>

      <section>
        <h2>요일별 활동</h2>
        {#if report.activity_by_weekday.length === 0}<p class="muted">없음</p>{:else}
        <Chart type="bar" data={weekdayData} options={barOpts} height={200} />
        <details class="exact">
          <summary>정확한 수치</summary>
          <ul class="kv">
            {#each report.activity_by_weekday as w}<li><span>{w.weekday}</span><span class="num">{fmtInt(w.sessions)}</span></li>{/each}
          </ul>
        </details>
        {/if}
      </section>

      <section>
        <h2>토큰 상위 세션</h2>
        {#if report.token_leaders.length === 0}<p class="muted">없음</p>{:else}
        <ul class="leaders">
          {#each report.token_leaders as s}
            <li>
              <div class="lead-top">
                <span class="mono" title={s.session_id}>{s.session_id.slice(0, 26)}</span>
                <span>[{s.date ?? "-"}]</span>
                <span class="num">{fmtInt(s.tokens)} tok</span>
              </div>
              {#if s.prompt}<div class="lead-prompt">{s.prompt}</div>{/if}
            </li>
          {/each}
        </ul>
        {/if}
      </section>
    </div>

    <section>
      <h2>시간 버킷 추세 ({by})</h2>
      {#if report.time_buckets.length === 0}<p class="muted">없음</p>{:else}
      <div class="trend"><Chart type="line" data={trendData} options={lineOpts} height={200} /></div>
      <table class="buckets">
        <thead><tr><th>구간</th><th class="r">세션</th><th class="r">토큰</th><th>대표 스킬</th><th>최빈 단어</th></tr></thead>
        <tbody>
          {#each report.time_buckets as b}
            <tr>
              <td>{b.label}</td>
              <td class="r">{fmtInt(b.sessions)}</td>
              <td class="r">{fmtInt(b.tokens)}</td>
              <td>{b.top_skill ?? "-"}{#if b.top_skill_calls} ({fmtInt(b.top_skill_calls)}){/if}</td>
              <td class="words">{b.top_words.length ? b.top_words.join(", ") : "-"}</td>
            </tr>
          {/each}
        </tbody>
      </table>
      {/if}
    </section>

    <section>
      <h2>자주 쓴 단어 — {report.top_words.map((s) => WORDS_LABEL[s.category] ?? s.category).join(" / ")}</h2>
      {#each report.top_words as sec}
        <div class="word-section">
          <div class="word-cat">{WORDS_LABEL[sec.category] ?? sec.category}</div>
          {#if sec.words.length === 0}<span class="muted">없음</span>{:else}
          <div class="word-cloud">
            {#each sec.words as w}
              <span class="word" style="font-size:{12 + Math.min(16, w.count / Math.max(1, sec.words[0].count) * 16)}px">{w.word}<sub>{fmtInt(w.count)}</sub></span>
            {/each}
          </div>
          {/if}
        </div>
      {/each}
    </section>
  {/if}
  {:else if tab === "archive"}
    <Archive />
  {:else if tab === "restore"}
    <Restore />
  {:else if tab === "compact"}
    <Compact />
  {:else if tab === "summarize"}
    <Summarize />
  {:else if tab === "settings"}
    <Settings />
  {/if}
</main>

<style>
  :global(html, body) {
    background: #14161a; color: #e6e8eb;
    font-family: Inter, -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
    margin: 0;
  }
  main { max-width: 1100px; margin: 0 auto; padding: 24px 20px 60px; }
  header h1 { margin: 0 0 12px; font-size: 22px; }
  .tabs { display: flex; gap: 6px; margin-bottom: 16px; border-bottom: 1px solid #262d34; padding-bottom: 8px; }
  .tabs button { background: transparent; border: none; color: #9aa1a8; padding: 6px 12px; border-radius: 6px 6px 0 0; cursor: pointer; font-size: 13px; }
  .tabs button.active { color: #e6e8eb; background: #1b2026; }
  .controls { display: flex; flex-wrap: wrap; gap: 10px; align-items: end; }
  .controls label { display: flex; flex-direction: column; font-size: 12px; color: #9aa1a8; gap: 4px; }
  input, select {
    background: #1f242b; color: #e6e8eb; border: 1px solid #2c333b; border-radius: 6px;
    padding: 6px 8px; font-size: 13px; width: 92px;
  }
  button { background: #2a3138; color: #e6e8eb; border: 1px solid #3a434c; border-radius: 6px; padding: 7px 12px; cursor: pointer; font-size: 13px; }
  button:disabled { opacity: 0.5; cursor: default; }
  button.primary { background: #2f6fed; border-color: #4a82f5; }
  button.scan { background: #1f7a4d; border-color: #2a9c62; }
  .scan-bar { margin-top: 14px; }
  .scan-msg { font-size: 12px; color: #9aa1a8; margin-bottom: 4px; }
  .scan-msg .warn { color: #e0a458; }
  .bar { height: 8px; background: #1f242b; border-radius: 4px; overflow: hidden; }
  .fill { height: 100%; background: #2a9c62; transition: width 0.15s; }
  .error { background: #3a1f1f; border: 1px solid #6b2a2a; color: #ffb4b4; padding: 10px 12px; border-radius: 6px; margin: 16px 0; }
  .empty, .note, .muted { color: #9aa1a8; font-size: 14px; }
  .note { background: #2a2a1f; border: 1px solid #5a5a2a; padding: 8px 12px; border-radius: 6px; margin: 14px 0; }
  .cards { display: grid; grid-template-columns: repeat(auto-fill, minmax(120px, 1fr)); gap: 10px; margin: 18px 0 8px; }
  .card { background: #1b2026; border: 1px solid #262d34; border-radius: 8px; padding: 12px; }
  .card .k { font-size: 11px; color: #9aa1a8; }
  .card .v { font-size: 20px; font-weight: 600; margin-top: 4px; }
  .range { font-size: 13px; color: #9aa1a8; margin-bottom: 16px; }
  .grid { display: grid; grid-template-columns: 1fr 1fr; gap: 16px; }
  section { background: #1b2026; border: 1px solid #262d34; border-radius: 8px; padding: 14px 16px; margin-bottom: 16px; }
  h2 { font-size: 14px; margin: 0 0 10px; color: #c9ced3; }
  .num { width: 70px; text-align: right; color: #9aa1a8; font-variant-numeric: tabular-nums; }
  .exact { margin-top: 10px; font-size: 12px; color: #9aa1a8; }
  .exact > summary { cursor: pointer; list-style: none; }
  .exact > summary::before { content: "▸ "; }
  .exact[open] > summary::before { content: "▾ "; }
  .kv { list-style: none; margin: 8px 0 0; padding: 0; display: grid; grid-template-columns: 1fr auto; gap: 2px 12px; }
  .kv li { display: contents; }
  table { width: 100%; border-collapse: collapse; font-size: 13px; }
  th { text-align: left; color: #9aa1a8; font-weight: 500; padding: 4px 6px; border-bottom: 1px solid #2c333b; }
  td { padding: 4px 6px; border-bottom: 1px solid #22282e; }
  .r { text-align: right; }
  .mono { font-family: ui-monospace, monospace; }
  ul.leaders { list-style: none; margin: 0; padding: 0; }
  ul.leaders li { margin-bottom: 10px; font-size: 13px; }
  .lead-top { display: flex; gap: 8px; align-items: baseline; }
  .lead-prompt { color: #9aa1a8; font-size: 12px; margin-top: 2px; }
  .buckets td.words { color: #c9ced3; }
  .trend { margin-bottom: 12px; }
  .word-section { margin-bottom: 10px; }
  .word-cat { font-size: 12px; color: #9aa1a8; margin-bottom: 4px; }
  .word-cloud { display: flex; flex-wrap: wrap; gap: 6px 12px; align-items: baseline; }
  .word { color: #e6e8eb; }
  .word sub { color: #6b7178; font-size: 10px; }
  @media (max-width: 760px) { .grid { grid-template-columns: 1fr; } }
</style>
