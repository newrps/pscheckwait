// API base — in production same origin; in dev Vite proxies /api
const API = "/api";

export function getToken() {
  if (typeof localStorage === "undefined") return "";
  return localStorage.getItem("pscheckwait.admin.token") || "";
}

export function setToken(t) {
  localStorage.setItem("pscheckwait.admin.token", t);
}

export function clearToken() {
  localStorage.removeItem("pscheckwait.admin.token");
}

async function request(method, path, body, opts = {}) {
  const headers = { "Content-Type": "application/json" };
  if (opts.auth !== false) {
    headers.Authorization = `Bearer ${getToken()}`;
  }
  const res = await fetch(`${API}${path}`, {
    method,
    headers,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  if (res.status === 401) throw new Error("토큰이 잘못되었습니다");
  if (res.status === 404) throw new Error("없는 항목입니다");
  if (res.status === 409) throw new Error("이미 존재합니다");
  if (res.status === 503) throw new Error("초기 설정이 필요합니다");
  if (!res.ok) throw new Error(`요청 실패 (${res.status})`);
  if (res.status === 204) return null;
  const ct = res.headers.get("content-type") || "";
  return ct.includes("json") ? res.json() : res.text();
}

// Public
export const isFirstRun = async () => {
  try {
    const r = await fetch(`${API}/admin/sites`, {
      headers: { Authorization: "Bearer __probe__" },
    });
    return r.status === 503;
  } catch (_) {
    return false;
  }
};

export const setup = (body) => request("POST", "/setup", body, { auth: false });

// Admin
export const listSites = () => request("GET", "/admin/sites");
export const getSite = (d) => request("GET", `/admin/sites/${encodeURIComponent(d)}`);
export const createSite = (b) => request("POST", "/admin/sites", b);
export const updateSite = (d, b) => request("PUT", `/admin/sites/${encodeURIComponent(d)}`, b);
export const deleteSite = (d) => request("DELETE", `/admin/sites/${encodeURIComponent(d)}`);
export const listQueue = (d, opts = {}) => {
  const params = new URLSearchParams();
  if (opts.limit != null) params.set("limit", opts.limit);
  if (opts.offset != null) params.set("offset", opts.offset);
  const qs = params.toString();
  return request(
    "GET",
    `/admin/sites/${encodeURIComponent(d)}/queue${qs ? `?${qs}` : ""}`
  );
};
export const kickToken = (d, t) =>
  request("POST", `/admin/sites/${encodeURIComponent(d)}/kick?token=${encodeURIComponent(t)}`);
export const admitToken = (d, t) =>
  request("POST", `/admin/sites/${encodeURIComponent(d)}/admit?token=${encodeURIComponent(t)}`);
export const clearQueue = (d) =>
  request("POST", `/admin/sites/${encodeURIComponent(d)}/clear`);

// Admin WebSocket: returns the WebSocket instance
export function adminWebSocket() {
  const proto = location.protocol === "https:" ? "wss:" : "ws:";
  const url = `${proto}//${location.host}/api/admin/ws?token=${encodeURIComponent(getToken())}`;
  return new WebSocket(url);
}
