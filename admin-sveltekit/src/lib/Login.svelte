<script>
  import { listSites, setToken } from "./api.js";

  let { onSuccess } = $props();

  let token = $state("");
  let err = $state("");
  let loading = $state(false);

  async function connect() {
    if (!token.trim()) { err = "토큰을 입력하세요"; return; }
    err = "";
    loading = true;
    try {
      setToken(token.trim());
      await listSites();
      onSuccess();
    } catch (e) {
      err = e.message;
    } finally {
      loading = false;
    }
  }
</script>

<div class="login-row">
  <input
    type="password"
    placeholder="관리자 토큰"
    bind:value={token}
    on:keydown={(e) => e.key === "Enter" && connect()}
  />
  <button class="btn" style="width:100%; margin-top:8px" disabled={loading} on:click={connect}>
    {loading ? "접속중..." : "접속"}
  </button>
  {#if err}
    <div class="hint" style="color:var(--danger); margin-top:6px">{err}</div>
  {/if}
</div>

<style>
  .login-row { margin-bottom: 16px; }
  input {
    width: 100%;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 8px 10px;
    color: var(--text);
    font-size: 13px;
  }
</style>
