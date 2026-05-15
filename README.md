# Pscheckwait

스크립트 한 줄로 붙이는 멀티테넌트 **가상 대기열** 시스템.
단일 실행파일 + 웹 설치 위저드 + 웹 관리자.

```
┌────────────────────┐
│  고객 사이트         │  <script src=".../pscheckwait.js"> 한 줄
└──────────┬─────────┘
           │ WebSocket (fallback: HTTP polling)
           ▼
┌────────────────────┐       ┌──────────┐
│ Pscheckwait 서버 🦀  │ ───▶  │  Redis   │ (선택)
│  도메인별 큐 + 설정   │       └──────────┘
└────────────────────┘
```

- **언어**: Rust(axum) + SvelteKit
- **배포**: 단일 바이너리, 외부 의존성 0개 (Redis는 선택)
- **확장**: 멀티테넌트 (도메인별 독립 큐)
- **실시간**: WebSocket 기반, polling 폴백

---

## 빠른 시작

### Windows
1. `dist/pscheckwait-server.exe` 다운로드 → 더블클릭
2. 브라우저 자동 오픈 → 관리자 토큰 설정
3. 사이트 추가 → 설치 스니펫 복사

### Linux (x86_64)
```bash
chmod +x pscheckwait-server-linux
./pscheckwait-server-linux
# http://서버주소:3000 접속
```

설정/데이터는 실행파일 옆 `data/` 폴더에 저장됨.

---

## 문서

| 문서 | 내용 |
|---|---|
| **[GUIDE.md](GUIDE.md)** | 사용 가이드 — 설치, 사이트 등록, 큐 관리, Origin 검증, Redis, 트러블슈팅 |
| **[DEPLOY.md](DEPLOY.md)** | 운영 배포 — Oracle Cloud + Caddy + Redis + 자동백업 실전 셋업 |

---

## 빌드

Rust 1.83+ 필요.

```bash
cd server
cargo build --release
# → target/release/pscheckwait-server[.exe]
```

**Linux 크로스 빌드 (Docker)**:
```bash
docker run --rm -v "$(pwd)/..:/work" -w /work/server rust:1-bookworm \
  cargo build --release --target-dir target-linux
```

---

## 프로젝트 구조

```
pscheckwait/
├── server/                Rust(axum) 백엔드
│   ├── src/main.rs        라우터 + 핸들러 + 임베드 자산
│   └── src/backend.rs     Backend trait (Memory + Redis)
├── admin-sveltekit/       관리자 SPA (build 산출물은 서버에 임베드)
├── loader/pscheckwait.js  고객 사이트 주입용 JS 로더
├── wait/index.html        대기 화면 (서버 임베드)
├── demo/index.html        테스트 데모 페이지
└── dist/                  배포 바이너리
```

정적 파일은 `include_str!` / `rust-embed` 으로 .exe에 임베드됨.

---

## 환경 변수

| 변수 | 기본값 | 설명 |
|---|---|---|
| `PSCHECKWAIT_PORT` | `3000` | 리스닝 포트 |
| `PSCHECKWAIT_BIND` | `0.0.0.0` | 바인딩 주소 (운영: `127.0.0.1`) |
| `PSCHECKWAIT_DATA_DIR` | `./data` | config/sites 저장 경로 |
| `PSCHECKWAIT_REDIS_URL` | — | 설정시 Redis 사용, 미설정시 인메모리 |
| `PSCHECKWAIT_ADMIN_TOKEN` | — | 위저드 건너뛰고 토큰 강제 지정 |
| `PSCHECKWAIT_NO_BROWSER` | `0` | `1` 이면 시작시 브라우저 자동오픈 안함 |

우선순위: **환경변수 > config.json**

---

## API

### 공개 (로더 사용)

| Method | Path | 설명 |
|---|---|---|
| `POST` | `/api/enter?site=` | 큐 진입, 토큰 발급 |
| `GET`  | `/api/status?site=&token=` | 순번 조회 (polling) |
| `POST` | `/api/heartbeat?site=&token=` | 생존 신호 (polling) |
| `POST` | `/api/leave?site=&token=` | 자발적 이탈 |
| `GET`  | `/api/stats?site=` | 사이트 상태 |
| `GET`  | `/api/ws?site=&token=` | **WebSocket** 실시간 push |
| `GET`  | `/api/health` | 헬스체크 |

### 관리자 (Bearer 토큰)

| Method | Path | 설명 |
|---|---|---|
| `GET`    | `/api/admin/sites` | 사이트 목록 |
| `POST`   | `/api/admin/sites` | 사이트 추가 |
| `GET`    | `/api/admin/sites/:domain` | 사이트 상세 |
| `PUT`    | `/api/admin/sites/:domain` | 설정 수정 |
| `DELETE` | `/api/admin/sites/:domain` | 사이트 제거 |
| `GET`    | `/api/admin/sites/:domain/queue` | 큐 명단 (페이지네이션) |
| `POST`   | `/api/admin/sites/:domain/admit?token=` | 강제 입장 |
| `POST`   | `/api/admin/sites/:domain/kick?token=` | 추방 |
| `POST`   | `/api/admin/sites/:domain/clear` | 큐 비우기 |
| `GET`    | `/api/admin/ws` | 관리자 실시간 stats push |

### 초기 설정 (토큰 미설정시)

| Method | Path | 설명 |
|---|---|---|
| `POST` | `/api/setup` | 관리자 토큰 + Redis URL 저장 |

---

## 관리자 콘솔

- 사이트 추가/삭제 (도메인만 입력)
- 사이트별 설정
  - `max_active` (동시 입장 인원)
  - 대기 화면 문구 (제목/부제/순번 라벨/메타 포맷)
  - Origin 검증 ON/OFF + 허용 도메인 리스트
- 실시간 통계 (WebSocket push)
- 큐 명단 + 강제 입장/추방 (페이지네이션)
- 사이트별 설치 스니펫 자동 생성

---

## 로드맵

- [ ] 사이트별 통계 차트 (입장/대기 시계열)
- [ ] 봇 차단 (캡차)
- [ ] 와일드카드 도메인 (`*.example.com`)
- [ ] Redis Pub/Sub 다중 서버 promote 조율
- [ ] WS 이벤트 기반 broadcast (지금은 2초 tick)
- [ ] 관리자 콘솔에서 서버 설정 수정

---

## 라이선스

MIT
