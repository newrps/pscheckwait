<script>
  import { setup, setToken } from "./api.js";

  let { onDone } = $props();

  let token = $state("");
  let useRedis = $state(false);
  let redisUrl = $state("");
  let msg = $state(null);
  let saving = $state(false);
  let showToken = $state(false);

  function gen() {
    const bytes = new Uint8Array(24);
    crypto.getRandomValues(bytes);
    token = btoa(String.fromCharCode(...bytes)).replace(/[/+=]/g, "").slice(0, 32);
    showToken = true;
  }

  async function save() {
    msg = null;
    const t = token.trim();
    if (!t) { msg = { kind: "err", text: "관리자 토큰을 입력하세요" }; return; }
    if (t.length < 8) { msg = { kind: "err", text: "토큰은 8자 이상이어야 합니다" }; return; }
    saving = true;
    try {
      const body = await setup({
        admin_token: t,
        redis_url: useRedis ? redisUrl.trim() : "",
      });
      setToken(t);
      if (body.restart_required) {
        msg = { kind: "ok", text: "저장됨. Redis 적용을 위해 서버를 재시작하세요." };
      } else {
        msg = { kind: "ok", text: "저장됨. 관리자로 이동합니다..." };
        setTimeout(onDone, 800);
      }
    } catch (e) {
      msg = { kind: "err", text: e.message };
    } finally {
      saving = false;
    }
  }
</script>

<div class="wrap">
  <div class="card">
    <div class="logo">Pscheckwait</div>
    <h1>처음 오신 것을 환영합니다 👋</h1>
    <p class="sub">관리자 토큰을 설정하세요. 이 토큰으로 관리자 콘솔에 접근합니다.</p>

    <div class="field">
      <label for="token">관리자 토큰 <span style="color:var(--danger)">*</span></label>
      <div class="input-row">
        <input
          id="token"
          class="form-input"
          type={showToken ? "text" : "password"}
          bind:value={token}
          placeholder="긴 무작위 문자열 권장"
          on:keydown={(e) => e.key === "Enter" && save()}
        />
        <button class="btn secondary" type="button" on:click={gen}>생성</button>
      </div>
      <div class="hint">최소 8자. 잃어버리면 <code>data/config.json</code> 삭제 후 재시작.</div>
    </div>

    <div class="field">
      <label class="toggle">
        <input type="checkbox" bind:checked={useRedis} />
        <span>Redis 백엔드 사용 (선택)</span>
      </label>
      {#if useRedis}
        <input
          class="form-input"
          style="margin-top:8px"
          bind:value={redisUrl}
          placeholder="redis://127.0.0.1:6379"
        />
        <div class="hint">설정 후 서버 재시작 필요.</div>
      {/if}
    </div>

    <button class="btn" style="width:100%" disabled={saving} on:click={save}>
      {saving ? "저장중..." : "시작하기"}
    </button>

    {#if msg}
      <div class="msg" class:ok={msg.kind === "ok"} class:err={msg.kind === "err"}>
        {msg.text}
      </div>
    {/if}

    <div class="footer">
      <code>data/config.json</code>에 저장. 환경변수 <code>PSCHECKWAIT_ADMIN_TOKEN</code> 가 있으면 그쪽 우선.
    </div>
  </div>
</div>

<style>
  .wrap {
    min-height: 100vh;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
    background: linear-gradient(135deg, #0f172a 0%, #1e293b 100%);
  }
  .card {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 16px;
    padding: 40px 44px;
    max-width: 520px;
    width: 100%;
    box-shadow: 0 20px 60px rgba(0,0,0,0.4);
  }
  .logo {
    font-size: 36px; font-weight: 800; margin-bottom: 4px;
    background: linear-gradient(135deg, var(--accent) 0%, #818cf8 100%);
    -webkit-background-clip: text;
    background-clip: text;
    -webkit-text-fill-color: transparent;
  }
  h1 { font-size: 22px; margin: 0 0 6px; }
  .sub { color: var(--muted); font-size: 14px; margin: 0 0 28px; line-height: 1.5; }
  .field { margin-bottom: 20px; }
  .input-row { display: flex; gap: 8px; }
  .input-row .btn { white-space: nowrap; }
  .toggle { display: flex; align-items: center; gap: 10px; cursor: pointer; font-size: 14px; color: var(--text); margin-bottom: 0; }
  .toggle input { margin: 0; }
  .msg {
    margin-top: 14px;
    padding: 10px 12px;
    border-radius: 8px;
    font-size: 13px;
  }
  .msg.err { background: rgba(248,113,113,0.15); color: var(--danger); }
  .msg.ok { background: rgba(74,222,128,0.15); color: var(--ok); }
  .footer {
    margin-top: 24px;
    padding-top: 20px;
    border-top: 1px solid var(--border);
    font-size: 12px;
    color: var(--muted);
    text-align: center;
  }
</style>
