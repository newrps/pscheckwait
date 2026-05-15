// k6 load test — 10000 VU stress
//   docker run --rm -i --ulimit nofile=65536:65536 -v ${PWD}:/work grafana/k6 run /work/loadtest.js

import http from 'k6/http';
import { check, sleep } from 'k6';

const BASE = 'https://queue.zam.kr/api';
const SITE = 'fishing.zam.kr';
const ORIGIN = 'https://fishing.zam.kr';

export const options = {
  stages: [
    { duration: '30s',  target: 2000 },
    { duration: '60s',  target: 5000 },
    { duration: '60s',  target: 10000 },
    { duration: '30s',  target: 10000 },
    { duration: '15s',  target: 0 },
  ],
  thresholds: {
    http_req_failed: ['rate<0.20'],
    http_req_duration: ['p(95)<3000'],
  },
};

export default function () {
  const headers = { 'Origin': ORIGIN };

  const r1 = http.post(`${BASE}/enter?site=${SITE}`, null, { headers });
  check(r1, { 'enter ok': (r) => r.status === 200 });
  if (r1.status !== 200) return;

  let token = null;
  try { token = r1.json('token'); } catch (_) {}
  if (!token) return;

  for (let i = 0; i < 5; i++) {
    sleep(3);
    const r2 = http.post(`${BASE}/heartbeat?site=${SITE}&token=${token}`, null, { headers });
    check(r2, { 'heartbeat ok': (r) => r.status === 200 });
  }

  http.post(`${BASE}/leave?site=${SITE}&token=${token}`, null, { headers });
}
