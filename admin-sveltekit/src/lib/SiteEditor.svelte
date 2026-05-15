<script>
  import { getSite, updateSite, deleteSite } from "./api.js";

  let { domain, onDeleted, onChanged } = $props();

  let info = $state(null);
  let max_active = $state(3);
  let title = $state("");
  let subtitle = $state("");
  let position_label = $state("");
  let meta_format = $state("");
  let verify_origin = $state(true);
  let allowed_origins = $state("");

  let toast = $state(null);

  async function load() {
    info = await getSite(domain);
    const c = info.config;
    max_active = c.max_active;
    title = c.title || "";
    subtitle = c.subtitle || "";
    position_label = c.position_label || "";
    meta_format = c.meta_format || "";
    verify_origin = c.verify_origin !== false;
    allowed_origins = (c.allowed_origins || []).join(", ");
  }

  async function refreshStats() {
    if (!domain) return;
    try {
      const r = await getSite(domain);
      if (info) {
        info = { ...info, active: r.active, waiting: r.waiting };
      }
    } catch (_) {}
  }

  $effect(() => {
    domain;
    load();
    const id = setInterval(refreshStats, 2000);
    return () => clearInterval(id);
  });

  async function save() {
    try {
      await updateSite(domain, {
        max_active: Number(max_active),
        title,
        subtitle,
        position_label,
        meta_format,
        verify_origin,
        allowed_origins: allowed_origins.split(",").map((s) => s.trim()).filter(Boolean),
      });
      toast = { kind: "ok", text: "저장됨" };
      onChanged?.();
    } catch (e) {
      toast = { kind: "err", text: e.message };
    }
    setTimeout(() => (toast = null), 2000);
  }

  async function del() {
    if (!confirm(`"${domain}" 삭제? 큐도 함께 비워집니다.`)) return;
    await deleteSite(domain);
    onDeleted?.();
  }

  const snippet = (origin) =>
    `<!-- 옵션 A: Pscheckwait 서버에서 직접 호스팅 (자동 설정) -->
<script src="${origin}/loader/pscheckwait.js"><\/script>

<!-- 옵션 B: CDN/자체 서버 호스팅 + 서버 주소 명시 -->
<script src="https://your-cdn.example.com/pscheckwait.js"
        data-server="${origin}"><\/script>`;
</script>

{#if info}
  <h2 class="dom">{domain}</h2>
  <p class="page-sub">이 사이트의 큐와 대기 화면 설정</p>

  <div class="panel">
    <h3>실시간 상태</h3>
    <div class="stats">
      <div class="stat"><div class="val">{info.active}</div><div class="lbl">입장중</div></div>
      <div class="stat"><div class="val">{info.waiting}</div><div class="lbl">대기중</div></div>
      <div class="stat"><div class="val">{max_active}</div><div class="lbl">최대 입장</div></div>
    </div>
  </div>

  <div class="panel">
    <h3>설정</h3>
    <div class="row">
      <label for="max">최대 동시 입장 인원</label>
      <input id="max" type="number" min="1" class="form-input" bind:value={max_active} />
    </div>
    <div class="row">
      <label for="t">대기 화면 제목</label>
      <input id="t" class="form-input" bind:value={title} />
    </div>
    <div class="row">
      <label for="s">대기 화면 부제</label>
      <input id="s" class="form-input" bind:value={subtitle} />
    </div>
    <div class="row">
      <label for="pl">순번 라벨</label>
      <input id="pl" class="form-input" bind:value={position_label} />
    </div>
    <div class="row">
      <label for="mf">메타 정보 포맷</label>
      <input id="mf" class="form-input" bind:value={meta_format} />
      <div class="hint">치환자: <code>{"{waiting}"}</code> <code>{"{active}"}</code> <code>{"{position}"}</code></div>
    </div>
    <div class="row">
      <label class="toggle">
        <input type="checkbox" bind:checked={verify_origin} />
        <span>Origin 검증 사용</span>
      </label>
    </div>
    <div class="row">
      <label for="ao">허용 도메인 (콤마 구분)</label>
      <input id="ao" class="form-input" bind:value={allowed_origins} placeholder="example.com, www.example.com" />
      <div class="hint">비워두면 사이트 도메인만 자동 허용.</div>
    </div>
    <div class="actions">
      <button class="btn" on:click={save}>저장</button>
      <a class="btn secondary" href="/demo/" target="_blank" rel="noopener">미리보기</a>
      <button class="btn danger" style="margin-left:auto" on:click={del}>사이트 삭제</button>
      {#if toast}
        <span class="toast {toast.kind}">{toast.text}</span>
      {/if}
    </div>
  </div>

  <div class="panel">
    <h3>설치 코드</h3>
    <div class="hint" style="margin-bottom:8px">아래를 사이트 <code>&lt;head&gt;</code>에 붙여넣으세요.</div>
    <pre class="snippet">{snippet(window.location.origin)}</pre>
  </div>
{/if}

<style>
  .dom { font-size: 24px; margin: 0 0 4px; }
  .page-sub { color: var(--muted); margin: 0 0 24px; font-size: 13px; }
  .stats { display: grid; grid-template-columns: repeat(3, 1fr); gap: 12px; }
  .stat {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 14px;
    text-align: center;
  }
  .stat .val { font-size: 28px; font-weight: 700; color: var(--accent); font-variant-numeric: tabular-nums; }
  .stat .lbl { font-size: 10px; color: var(--muted); text-transform: uppercase; letter-spacing: 0.05em; margin-top: 4px; }
  .toggle { display: flex; align-items: center; gap: 8px; cursor: pointer; color: var(--text); margin-bottom: 0; }
  .toggle input { margin: 0; }
  .actions { display: flex; gap: 8px; align-items: center; }
  .actions a { text-decoration: none; display: inline-flex; align-items: center; }
  .toast {
    margin-left: auto;
    font-size: 12px;
  }
  .toast.ok { color: var(--ok); }
  .toast.err { color: var(--danger); }
  .snippet {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 12px;
    font-family: Consolas, Monaco, monospace;
    font-size: 12px;
    color: var(--text);
    overflow-x: auto;
    white-space: pre-wrap;
    margin: 0;
  }
</style>
