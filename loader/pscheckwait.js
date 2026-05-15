(function () {
  "use strict";

  const script = document.currentScript;

  // 1. Pscheckwait 서버 도메인 결정:
  //    우선순위: data-server > 스크립트 src에서 추론 > 현재 origin
  const SERVER_BASE = (function () {
    if (script && script.dataset.server) {
      return script.dataset.server.replace(/\/+$/, "");
    }
    const src = script && script.src ? script.src : "";
    try {
      const u = new URL(src);
      return `${u.protocol}//${u.host}`;
    } catch (_) {
      return location.origin;
    }
  })();

  // 2. API 베이스: data-api 또는 기본 {server}/api
  const API_BASE = (script && script.dataset.api) || `${SERVER_BASE}/api`;

  // 3. 대기 페이지 URL: data-wait 또는 기본 {server}/wait
  const WAIT_PAGE = (function () {
    const p = script && script.dataset.wait;
    if (!p) return `${SERVER_BASE}/wait`;
    if (/^https?:\/\//i.test(p)) return p;
    return `${SERVER_BASE}${p.startsWith("/") ? "" : "/"}${p}`;
  })();

  // 4. 사이트 식별자: data-site 또는 현재 도메인
  const SITE = (script && script.dataset.site) || location.hostname || "unknown";

  // 5. 디버그 모드: data-debug="1"이면 콘솔에 상세 로그
  const DEBUG = !!(script && script.dataset.debug);

  // 진단용: 페이지에서 window.__pscheckwait__ 로 항상 접근 가능
  window.__pscheckwait__ = {
    version: "0.1.0",
    server: SERVER_BASE,
    api: API_BASE,
    wait_page: WAIT_PAGE,
    site: SITE,
    token: null,
    status: "init",
    config: null,
  };

  function dbg(...args) {
    if (DEBUG) console.log("[Pscheckwait]", ...args);
  }
  dbg("init", window.__pscheckwait__);
  const HEARTBEAT_INTERVAL_MS = 15000;
  const IDLE_TIMEOUT_MS = parseInt(script && script.dataset.idleTimeout, 10) > 0
    ? parseInt(script.dataset.idleTimeout, 10) * 1000
    : 180000;
  const STORAGE_KEY = `pscheckwait.token.${SITE}`;

  function url(path, params) {
    const u = new URL(`${API_BASE}${path}`);
    u.searchParams.set("site", SITE);
    if (params) for (const [k, v] of Object.entries(params)) u.searchParams.set(k, v);
    return u.toString();
  }

  async function postEnter() {
    const res = await fetch(url("/enter"), { method: "POST" });
    if (res.status === 404) return { unknown_site: true };
    if (!res.ok) throw new Error(`enter failed: ${res.status}`);
    return res.json();
  }

  async function getStatus(token) {
    const res = await fetch(url("/status", { token }));
    if (res.status === 404) return { unknown_token: true };
    if (!res.ok) throw new Error(`status failed: ${res.status}`);
    return res.json();
  }

  function sendHeartbeat(token) {
    try {
      fetch(url("/heartbeat", { token }), { method: "POST", keepalive: true });
    } catch (_) {}
  }

  function setupLeaveOnUnload(token) {
    const u = url("/leave", { token });
    window.addEventListener("pagehide", () => {
      if (navigator.sendBeacon) navigator.sendBeacon(u);
      else fetch(u, { method: "POST", keepalive: true }).catch(() => {});
    });
  }

  function restoreFromHash() {
    if (!location.hash) return null;
    const hash = new URLSearchParams(location.hash.slice(1));
    const token = hash.get("wl_token");
    if (!token) return null;
    sessionStorage.setItem(STORAGE_KEY, token);
    // Clean hash so URL looks normal
    hash.delete("wl_token");
    const remaining = hash.toString();
    const newHash = remaining ? `#${remaining}` : "";
    try {
      history.replaceState(null, "", location.pathname + location.search + newHash);
    } catch (_) {}
    return token;
  }

  function redirectToWait(token) {
    const returnUrl = location.href;
    const params = new URLSearchParams({ site: SITE, return: returnUrl });
    const target = `${WAIT_PAGE}?${params.toString()}#token=${encodeURIComponent(token)}`;
    location.replace(target);
  }

  function setupHeartbeat(token) {
    setInterval(() => sendHeartbeat(token), HEARTBEAT_INTERVAL_MS);
  }

  function setupIdleTimeout(token) {
    let lastActivity = Date.now();
    const bump = () => { lastActivity = Date.now(); };
    const events = ["mousemove", "mousedown", "keydown", "scroll", "touchstart", "wheel"];
    events.forEach((ev) => window.addEventListener(ev, bump, { passive: true }));

    const checker = setInterval(() => {
      if (Date.now() - lastActivity < IDLE_TIMEOUT_MS) return;
      clearInterval(checker);
      events.forEach((ev) => window.removeEventListener(ev, bump));
      dbg("idle timeout reached, leaving queue");
      try {
        const u = url("/leave", { token });
        if (navigator.sendBeacon) navigator.sendBeacon(u);
        else fetch(u, { method: "POST", keepalive: true }).catch(() => {});
      } catch (_) {}
      sessionStorage.removeItem(STORAGE_KEY);
      location.reload();
    }, 5000);
  }

  async function run() {
    // 1. Restore token from URL hash if just returned from wait page
    restoreFromHash();

    // 2. Try existing session token
    let token = sessionStorage.getItem(STORAGE_KEY);
    let state = null;

    if (token) {
      try {
        state = await getStatus(token);
        if (state.unknown_site) return; // site not registered → loader is inert
        if (state.unknown_token) {
          state = null;
          token = null;
          sessionStorage.removeItem(STORAGE_KEY);
        }
      } catch (_) {
        token = null;
        state = null;
      }
    }

    // 3. No valid token — request a new one
    if (!token) {
      let entered;
      try {
        entered = await postEnter();
      } catch (_) {
        return; // server unreachable: let page load (fail-open)
      }
      if (entered.unknown_site) return;
      token = entered.token;
      sessionStorage.setItem(STORAGE_KEY, token);
      state = entered;
    }

    // 진단 정보 업데이트
    window.__pscheckwait__.token = token;
    window.__pscheckwait__.status = state.status;
    window.__pscheckwait__.config = state.config || null;

    // 한 줄 요약 로그 (디버그 모드 아니어도 한번은 찍음)
    console.log(
      `[Pscheckwait] site=${SITE} server=${SERVER_BASE} status=${state.status}` +
      (state.status === "waiting" ? ` position=${state.position}` : "")
    );
    dbg("state", state);

    // 4. Branch by status
    if (state.status === "admitted") {
      setupLeaveOnUnload(token);
      setupHeartbeat(token);
      setupIdleTimeout(token);
      return; // page loads normally
    }

    if (state.status === "waiting") {
      // Hard redirect: customer page never executes further
      redirectToWait(token);
      return;
    }

    // unknown / unexpected → fail open
  }

  // Run as early as possible so customer page renders less before redirect
  if (document.readyState === "loading" && document.documentElement) {
    run();
  } else {
    run();
  }
})();
