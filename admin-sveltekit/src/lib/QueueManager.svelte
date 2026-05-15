<script>
  import { listQueue, kickToken, admitToken, clearQueue } from "./api.js";

  let { domain } = $props();

  const PAGE_SIZE = 100;

  let activeList = $state([]);
  let waitingList = $state([]);
  let totalActive = $state(0);
  let totalWaiting = $state(0);
  let offset = $state(0);
  let timer;
  let loading = $state(false);

  async function load() {
    if (!domain) return;
    loading = true;
    try {
      const r = await listQueue(domain, { limit: PAGE_SIZE, offset });
      activeList = r.active;
      waitingList = r.waiting;
      totalActive = r.total_active;
      totalWaiting = r.total_waiting;
    } catch (_) {}
    loading = false;
  }

  $effect(() => {
    // domain 바뀌면 offset 리셋 (페이지 1로)
    domain;
    offset = 0;
    load();
    if (timer) clearInterval(timer);
    timer = setInterval(load, 2000);
    return () => clearInterval(timer);
  });

  async function kick(t) { await kickToken(domain, t); load(); }
  async function admit(t) { await admitToken(domain, t); load(); }
  async function clearAll() {
    if (!confirm(`"${domain}" 큐 전부 비울까요?`)) return;
    await clearQueue(domain);
    offset = 0;
    load();
  }

  function loadMore() {
    offset += PAGE_SIZE;
    load();
  }
  function reset() {
    offset = 0;
    load();
  }

  function fmtIdle(s) {
    if (s < 60) return `${s}초`;
    const m = Math.floor(s / 60);
    const sec = s % 60;
    return sec === 0 ? `${m}분` : `${m}분 ${sec}초`;
  }

  const hasMore = $derived(offset + activeList.length < totalActive || offset + waitingList.length < totalWaiting);
  const showingActive = $derived(activeList.length);
  const showingWaiting = $derived(waitingList.length);
</script>

<div class="panel">
  <h3 style="display:flex;align-items:center;justify-content:space-between">
    <span>큐 관리</span>
    <button class="btn danger" style="font-size:11px;padding:4px 10px" on:click={clearAll}>전부 비우기</button>
  </h3>

  <div class="bar">
    <span class="bar-info">
      입장중 <strong>{totalActive.toLocaleString()}</strong> · 대기중 <strong>{totalWaiting.toLocaleString()}</strong>
    </span>
    <span class="bar-info" style="margin-left:auto">
      페이지 {Math.floor(offset / PAGE_SIZE) + 1}
      {#if offset > 0}
        · <button class="link" on:click={reset}>처음으로</button>
      {/if}
    </span>
  </div>

  <div style="margin-bottom:14px">
    <div class="sectlabel">
      입장중 <span class="count">({showingActive} / {totalActive.toLocaleString()})</span>
    </div>
    <div class="list">
      {#each activeList as t (t.token)}
        <div class="row-item">
          <span class="pos">●</span>
          <span class="tok" title={t.token}>{t.token.slice(0, 8)}…</span>
          <span class="idle">{fmtIdle(t.idle_secs)} 전</span>
          <button class="mini danger" on:click={() => kick(t.token)}>추방</button>
        </div>
      {:else}
        <div class="empty">{totalActive === 0 ? "입장중 사용자 없음" : "이 페이지엔 없음"}</div>
      {/each}
    </div>
  </div>

  <div>
    <div class="sectlabel">
      대기중 <span class="count">({showingWaiting} / {totalWaiting.toLocaleString()})</span>
    </div>
    <div class="list">
      {#each waitingList as t (t.token)}
        <div class="row-item">
          <span class="pos">#{t.position}</span>
          <span class="tok" title={t.token}>{t.token.slice(0, 8)}…</span>
          <span class="idle">{fmtIdle(t.idle_secs)} 전</span>
          <button class="mini ok" on:click={() => admit(t.token)}>입장</button>
          <button class="mini danger" on:click={() => kick(t.token)}>추방</button>
        </div>
      {:else}
        <div class="empty">{totalWaiting === 0 ? "대기중 사용자 없음" : "이 페이지엔 없음"}</div>
      {/each}
    </div>
  </div>

  {#if hasMore}
    <button class="btn secondary" style="width:100%; margin-top:12px" on:click={loadMore} disabled={loading}>
      더 보기 ({PAGE_SIZE}개 더 로드)
    </button>
  {/if}
</div>

<style>
  .sectlabel {
    font-size: 11px;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin-bottom: 6px;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .count {
    font-size: 10px;
    color: var(--muted);
    text-transform: none;
    letter-spacing: 0;
    font-weight: 400;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 12px;
    padding: 8px 10px;
    background: var(--bg);
    border-radius: 6px;
    font-size: 12px;
    color: var(--muted);
  }
  .bar-info strong { color: var(--accent); font-variant-numeric: tabular-nums; }
  .link {
    background: none;
    border: none;
    color: var(--accent);
    cursor: pointer;
    font-size: 12px;
    padding: 0;
    text-decoration: underline;
  }
  .list {
    max-height: 240px;
    overflow-y: auto;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
  }
  .row-item {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    font-size: 12px;
    border-bottom: 1px solid var(--border);
  }
  .row-item:last-child { border-bottom: 0; }
  .tok {
    font-family: Consolas, monospace;
    color: var(--text);
    flex: 0 0 100px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pos {
    font-size: 11px;
    color: var(--accent);
    font-weight: 600;
    min-width: 24px;
    text-align: center;
  }
  .idle { color: var(--muted); font-size: 11px; flex: 1; }
  .mini {
    font-size: 10px;
    padding: 3px 8px;
    border-radius: 4px;
    border: 1px solid var(--border);
    background: transparent;
    color: var(--text);
    cursor: pointer;
  }
  .mini.danger { color: var(--danger); border-color: var(--danger); }
  .mini.ok { color: var(--ok); border-color: var(--ok); }
  .empty { padding: 16px; text-align: center; color: var(--muted); font-size: 12px; }
</style>
