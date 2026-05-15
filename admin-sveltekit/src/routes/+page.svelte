<script>
  import { onMount } from "svelte";
  import { isFirstRun, getToken, clearToken, listSites } from "$lib/api.js";
  import Setup from "$lib/Setup.svelte";
  import Login from "$lib/Login.svelte";
  import SiteList from "$lib/SiteList.svelte";
  import SiteEditor from "$lib/SiteEditor.svelte";
  import QueueManager from "$lib/QueueManager.svelte";

  let phase = $state("checking"); // "checking" | "setup" | "login" | "ready"
  let selected = $state("");
  let refreshKey = $state(0);

  onMount(async () => {
    const first = await isFirstRun();
    if (first) {
      phase = "setup";
      return;
    }
    if (getToken()) {
      try {
        await listSites();
        phase = "ready";
        return;
      } catch (_) {
        clearToken();
      }
    }
    phase = "login";
  });

  function logout() {
    clearToken();
    phase = "login";
    selected = "";
  }

  function handleSiteDeleted() {
    selected = "";
    refreshKey++;
  }
</script>

{#if phase === "checking"}
  <div class="loading">로딩중...</div>
{:else if phase === "setup"}
  <Setup onDone={() => (phase = "ready")} />
{:else if phase === "login"}
  <div class="login-wrap">
    <div class="login-card">
      <h1>Pscheckwait</h1>
      <p class="sub">관리자 콘솔</p>
      <Login onSuccess={() => (phase = "ready")} />
    </div>
  </div>
{:else}
  <div class="layout">
    <aside>
      <h1 class="brand">Pscheckwait</h1>
      <p class="sub">관리자 콘솔</p>
      <div class="top-actions">
        <button class="btn secondary mini" on:click={logout}>로그아웃</button>
      </div>
      <SiteList bind:selected refreshKey={refreshKey} />
    </aside>

    <main>
      {#if !selected}
        <div class="empty">
          <div class="big">📋</div>
          <div>왼쪽에서 사이트를 선택하거나 추가하세요</div>
        </div>
      {:else}
        {#key selected}
          <SiteEditor
            domain={selected}
            onDeleted={handleSiteDeleted}
            onChanged={() => refreshKey++}
          />
          <QueueManager domain={selected} />
        {/key}
      {/if}
    </main>
  </div>
{/if}

<style>
  .loading { padding: 40px; text-align: center; color: var(--muted); }
  .login-wrap {
    min-height: 100vh;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
    background: linear-gradient(135deg, #0f172a 0%, #1e293b 100%);
  }
  .login-card {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 16px;
    padding: 32px 40px;
    max-width: 400px;
    width: 100%;
  }
  .login-card h1 {
    margin: 0;
    background: linear-gradient(135deg, var(--accent) 0%, #818cf8 100%);
    -webkit-background-clip: text;
    background-clip: text;
    -webkit-text-fill-color: transparent;
    font-size: 28px;
  }
  .login-card .sub { color: var(--muted); margin: 4px 0 24px; font-size: 13px; }

  .layout {
    display: grid;
    grid-template-columns: 280px 1fr;
    min-height: 100vh;
  }
  aside {
    background: var(--panel-2);
    border-right: 1px solid var(--border);
    padding: 20px;
    overflow-y: auto;
    max-height: 100vh;
    position: sticky;
    top: 0;
  }
  main { padding: 32px 40px; max-width: 800px; }
  .brand {
    font-size: 20px;
    margin: 0;
    background: linear-gradient(135deg, var(--accent) 0%, #818cf8 100%);
    -webkit-background-clip: text;
    background-clip: text;
    -webkit-text-fill-color: transparent;
  }
  .sub { color: var(--muted); margin: 0 0 12px; font-size: 12px; }
  .top-actions { margin-bottom: 16px; }
  .mini { font-size: 11px; padding: 4px 8px; }
  .empty {
    text-align: center;
    padding: 60px 20px;
    color: var(--muted);
    font-size: 14px;
  }
  .empty .big { font-size: 36px; margin-bottom: 12px; opacity: 0.4; }

  @media (max-width: 720px) {
    .layout { grid-template-columns: 1fr; }
    aside { max-height: none; position: static; border-right: 0; border-bottom: 1px solid var(--border); }
  }
</style>
