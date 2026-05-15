<script>
  import { listSites, createSite, adminWebSocket } from "./api.js";

  let { selected = $bindable(""), refreshKey = 0 } = $props();

  let sites = $state([]);
  let newDomain = $state("");
  let addMsg = $state("");
  let connected = $state(false);

  let ws = null;
  let retryTimer = null;

  async function loadFallback() {
    try {
      sites = await listSites();
    } catch (e) {
      addMsg = e.message;
    }
  }

  function connectWS() {
    try {
      ws = adminWebSocket();
    } catch (_) {
      loadFallback();
      return;
    }
    ws.addEventListener("open", () => {
      connected = true;
    });
    ws.addEventListener("message", (ev) => {
      try {
        const m = JSON.parse(ev.data);
        if (Array.isArray(m.sites)) sites = m.sites;
      } catch (_) {}
    });
    ws.addEventListener("close", () => {
      connected = false;
      ws = null;
      if (!retryTimer) {
        retryTimer = setTimeout(() => {
          retryTimer = null;
          connectWS();
        }, 3000);
      }
    });
    ws.addEventListener("error", () => {
      // close 이벤트도 발생하므로 재연결은 거기서 처리
    });
  }

  $effect(() => {
    refreshKey;
    connectWS();
    return () => {
      try { ws && ws.close(); } catch (_) {}
      ws = null;
      clearTimeout(retryTimer);
      retryTimer = null;
    };
  });

  async function add() {
    if (!newDomain.trim()) return;
    addMsg = "";
    try {
      await createSite({ domain: newDomain.trim() });
      const d = newDomain.trim().toLowerCase();
      newDomain = "";
      // WS가 다음 tick에 자동 갱신해줄 거지만 즉시 반영도
      await loadFallback();
      selected = d;
    } catch (e) {
      addMsg = e.message;
    }
  }
</script>

<ul class="list">
  {#each sites as s (s.domain)}
    <li
      class="item"
      class:selected={s.domain === selected}
      on:click={() => (selected = s.domain)}
      role="button"
      tabindex="0"
      on:keydown={(e) => e.key === "Enter" && (selected = s.domain)}
    >
      <span class="dom">{s.domain}</span>
      <span class="badge">{s.active}/{s.max_active} · {s.waiting}대기</span>
    </li>
  {/each}
  {#if sites.length === 0}
    <li class="empty">등록된 사이트 없음</li>
  {/if}
</ul>

<div class="conn">
  <span class="dot" class:on={connected}></span>
  <span class="conn-text">{connected ? "ws 실시간 연결" : "재연결중..."}</span>
</div>

<div class="add">
  <label for="newdom">새 사이트 추가</label>
  <input
    id="newdom"
    class="form-input"
    placeholder="예: example.com"
    bind:value={newDomain}
    on:keydown={(e) => e.key === "Enter" && add()}
  />
  <button class="btn" style="width:100%; margin-top:6px" on:click={add}>추가</button>
  {#if addMsg}
    <div class="hint" style="color:var(--danger)">{addMsg}</div>
  {/if}
</div>

<style>
  .list { list-style: none; margin: 0; padding: 0; }
  .item {
    padding: 10px 12px;
    border-radius: 6px;
    cursor: pointer;
    font-size: 13px;
    margin-bottom: 4px;
    border: 1px solid transparent;
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .item:hover { background: var(--bg); }
  .item.selected { background: var(--bg); border-color: var(--accent); }
  .dom { font-weight: 500; }
  .badge {
    font-size: 10px;
    background: var(--border);
    color: var(--muted);
    padding: 2px 6px;
    border-radius: 999px;
  }
  .item.selected .badge { color: var(--accent); }
  .empty { padding: 12px; text-align: center; color: var(--muted); font-size: 12px; }
  .add { margin-top: 16px; padding-top: 16px; border-top: 1px solid var(--border); }
  .conn {
    margin-top: 10px;
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 10px;
    color: var(--muted);
  }
  .dot {
    width: 6px; height: 6px;
    border-radius: 50%;
    background: var(--danger);
  }
  .dot.on { background: var(--ok); box-shadow: 0 0 4px var(--ok); }
</style>
