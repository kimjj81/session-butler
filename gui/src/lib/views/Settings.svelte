<script lang="ts">
  import { onMount } from "svelte";
  import { getConfig, setConfig, type ConfigView } from "$lib/api";

  let loading = $state(true);
  let running = $state(false);
  let error = $state<string | null>(null);
  let saved = $state(false);

  let codexSessions = $state("");
  let hermesSessions = $state("");
  let defaultArchiveDays = $state(30);
  let enabledCodex = $state(true);
  let enabledHermes = $state(false);
  let language = $state("ko");

  let savedTimer: ReturnType<typeof setTimeout> | undefined;

  function apply(c: ConfigView) {
    codexSessions = c.codex_sessions ?? "";
    hermesSessions = c.hermes_sessions ?? "";
    defaultArchiveDays = c.default_archive_days ?? 30;
    enabledCodex = c.enabled_codex ?? false;
    enabledHermes = c.enabled_hermes ?? false;
    language = c.language ?? "ko";
  }

  onMount(async () => {
    try {
      apply(await getConfig());
    } catch (e: any) {
      error = String(e);
    } finally {
      loading = false;
    }
  });

  async function save() {
    if (running) return;
    running = true;
    error = null;
    try {
      await setConfig({
        codex_sessions: codexSessions,
        default_archive_days: defaultArchiveDays,
        enabled_codex: enabledCodex,
        enabled_hermes: enabledHermes,
        language,
      });
      // 다시 로드해 서버 측 정규화 결과를 반영
      apply(await getConfig());
      saved = true;
      if (savedTimer) clearTimeout(savedTimer);
      savedTimer = setTimeout(() => (saved = false), 3000);
    } catch (e: any) {
      error = String(e);
    } finally {
      running = false;
    }
  }
</script>

<section>
  <h2>Settings</h2>

  {#if loading}
    <div class="muted">설정을 불러오는 중…</div>
  {:else}
    <div class="form">
      <label class="field wide">
        <span>Codex 세션 디렉토리</span>
        <input
          type="text"
          bind:value={codexSessions}
          disabled={running}
          placeholder="~/.codex/sessions"
        />
      </label>

      <label class="field">
        <span>기본 보존일수 (days)</span>
        <input
          type="number"
          min="0"
          bind:value={defaultArchiveDays}
          disabled={running}
        />
      </label>

      <label class="field">
        <span>언어</span>
        <select bind:value={language} disabled={running}>
          <option value="ko">한국어 (ko)</option>
          <option value="en">English (en)</option>
        </select>
      </label>

      <label class="check">
        <input type="checkbox" bind:checked={enabledCodex} disabled={running} />
        <span>Codex 수집 활성화</span>
      </label>

      <label class="check">
        <input type="checkbox" bind:checked={enabledHermes} disabled={running} />
        <span>Hermes 수집 활성화</span>
      </label>
    </div>

    <div class="note">
      인덱스 DB는 앱 데이터 디렉토리에 고정되어 있어 편집할 수 없습니다.
    </div>

    <div class="actions">
      <button class="run" onclick={save} disabled={running}>
        {running ? "저장 중…" : "저장"}
      </button>
      {#if saved}<span class="saved">저장됨</span>{/if}
    </div>
  {/if}

  {#if error}
    <div class="error">오류: {error}</div>
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
  h2 { font-size: 14px; margin: 0 0 12px; color: #c9ced3; }

  .muted { font-size: 13px; color: #9aa1a8; }

  .form {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px 16px;
    align-items: end;
  }
  .field {
    display: flex;
    flex-direction: column;
    font-size: 12px;
    color: #9aa1a8;
    gap: 4px;
  }
  .field.wide { grid-column: 1 / -1; }

  input[type="text"],
  input[type="number"],
  select {
    background: #1f242b;
    color: #e6e8eb;
    border: 1px solid #2c333b;
    border-radius: 6px;
    padding: 6px 8px;
    font-size: 13px;
    font-family: inherit;
  }
  input[type="text"] { width: 100%; }
  input[type="number"] { width: 140px; }
  select { width: fit-content; }
  input:disabled,
  select:disabled { opacity: 0.5; cursor: default; }

  .check {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    color: #e6e8eb;
    cursor: pointer;
  }
  input[type="checkbox"] { accent-color: #2f6fed; }

  .note {
    margin-top: 14px;
    font-size: 12px;
    color: #9aa1a8;
    background: #14161a;
    border: 1px solid #262d34;
    border-radius: 6px;
    padding: 8px 10px;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 14px;
  }
  button.run {
    background: #1f7a4d;
    color: #e6e8eb;
    border: 1px solid #2a9c62;
    border-radius: 6px;
    padding: 7px 16px;
    cursor: pointer;
    font-size: 13px;
  }
  button.run:disabled { opacity: 0.5; cursor: default; }
  .saved { font-size: 13px; color: #6bd49a; }

  .error {
    background: #3a1f1f;
    border: 1px solid #6b2a2a;
    color: #ffb4b4;
    padding: 10px 12px;
    border-radius: 6px;
    margin-top: 14px;
    font-size: 13px;
  }
</style>