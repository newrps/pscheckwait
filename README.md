# Pscheckwait

스크립트 한 줄로 붙이는 멀티테넌트 가상 대기열(virtual waiting room) 시스템.
**단일 실행파일 + 웹 설치 위저드 + 웹 관리자** 한방에.

```
┌────────────────────┐
│ 고객 사이트         │  ← <script src="...loader.js"> 한 줄 삽입
│ example.com        │     (사이트 = 도메인으로 식별)
└──────────┬─────────┘
           │ WebSocket (fallback: HTTP polling)
           ↓
┌────────────────────┐
│ Pscheckwait 서버 🦀   │  ← 단일 .exe (admin/loader/demo 임베드)
│ 사이트별 큐/설정    │     도메인별 독립된 큐 + 설정
└────────────────────┘
```

## 한방 설치

### Windows

1. `dist/pscheckwait-server.exe` 더블클릭
2. 브라우저 자동 오픈 → 초기 설정 페이지
3. 관리자 토큰 입력 → "시작하기"
4. 관리자 콘솔에서 사이트 추가 → 설치 스니펫 복사

### Linux (x86_64)

1. `dist/pscheckwait-server-linux` 서버에 업로드
2. `chmod +x pscheckwait-server-linux && ./pscheckwait-server-linux`
3. 브라우저에서 `http://서버주소:3000` 접속 → 동일하게 진행

### macOS / 다른 아키텍처

소스에서 빌드: `cd server && cargo build --release`

설정과 큐 데이터는 실행파일 옆 `data/` 폴더에 저장됨. 파일 옮길 땐 `data/`도 같이.

## 문서

- **[GUIDE.md](GUIDE.md)** — 일반 사용 가이드 (설치, 사이트 등록, 큐 관리, 트러블슈팅)
- **[DEPLOY.md](DEPLOY.md)** — 운영 배포 가이드 (Oracle Cloud + Caddy + Redis + 자동백업 실전 기록)
- **README.md** (현재 문서) — 프로젝트 개요 + API 명세

## 직접 빌드

Rust 1.83+ 설치된 경우 (의존성중 edition2024 필요):

```bash
cd server
cargo build --release
# 결과: target/release/pscheckwait-server[.exe]
```

**Docker로 Linux 바이너리 만들기**:

```bash
docker run --rm -v "$(pwd)/..:/work" -w /work/server rust:1-bookworm \
  cargo build --release --target-dir target-linux
```

## 폴더 구조

```
pscheckwait/
├── dist/                  배포용 단일 .exe
├── server/                Rust(axum) 큐 API 서버
│   ├── src/main.rs        라우터 + 핸들러 + 임베드 자산
│   └── src/backend.rs     Backend trait + Memory/Redis 구현
├── loader/pscheckwait.js       브라우저에 주입되는 JS 로더 (WS + polling fallback)
├── admin/
│   ├── index.html         관리자 콘솔
│   └── setup.html         초기 설치 위저드
└── demo/index.html        테스트용 데모 사이트
```

모든 정적 파일은 `include_str!`로 .exe에 임베드됨. 빌드 후엔 .exe 하나만 있으면 동작.

## 환경 변수 (선택)

설정 우선순위: **환경변수 > config.json**

| 변수 | 설명 |
|---|---|
| `PSCHECKWAIT_ADMIN_TOKEN` | 관리자 토큰 (설치 위저드 건너뛰기) |
| `PSCHECKWAIT_REDIS_URL` | Redis URL (예: `redis://127.0.0.1:6379`) |
| `PSCHECKWAIT_DATA_DIR` | 데이터 폴더 (기본: exe 옆 `./data`). `none`이면 비활성 |
| `PSCHECKWAIT_PORT` | 포트 (기본 3000) |
| `PSCHECKWAIT_NO_BROWSER` | `1`이면 시작시 브라우저 자동 오픈 안함 |

## Redis 사용 (선택)

기본은 인메모리. Redis 쓰면 큐 상태가 서버 재시작에 살아남고 다중 인스턴스 운영 가능.

```powershell
docker run -d --name pscheckwait-redis -p 6379:6379 redis:7-alpine
$env:PSCHECKWAIT_REDIS_URL = "redis://127.0.0.1:6379"
.\pscheckwait-server.exe
```

또는 초기 설정 위저드에서 "Redis 백엔드 사용" 체크 후 URL 입력 → 저장 → 서버 재시작.

## API

### 공개 (loader가 사용)

| 메서드 | 경로 | 설명 |
|---|---|---|
| POST | `/api/enter?site=` | 큐 진입, 토큰 발급 |
| GET | `/api/status?site=&token=` | 내 순서 조회 (polling fallback) |
| POST | `/api/heartbeat?site=&token=` | 생존 신호 (polling fallback) |
| POST | `/api/leave?site=&token=` | 자발적 이탈 |
| GET | `/api/stats?site=` | 사이트 상태 |
| GET | `/api/ws?site=&token=` | **WebSocket** — 실시간 상태 push |
| GET | `/api/health` | 헬스체크 |

### 관리자 (Bearer 토큰 필요)

| 메서드 | 경로 | 설명 |
|---|---|---|
| GET | `/api/admin/sites` | 전체 사이트 목록 |
| POST | `/api/admin/sites` | 사이트 추가 |
| GET | `/api/admin/sites/:domain` | 사이트 1개 (설정 + 통계) |
| PUT | `/api/admin/sites/:domain` | 설정 수정 |
| DELETE | `/api/admin/sites/:domain` | 사이트 제거 |
| GET | `/api/admin/sites/:domain/queue` | 입장/대기 명단 |
| POST | `/api/admin/sites/:domain/admit?token=` | 강제 입장 |
| POST | `/api/admin/sites/:domain/kick?token=` | 추방 |
| POST | `/api/admin/sites/:domain/clear` | 큐 비우기 |

### 초기 설정 (한번만, 토큰 미설정시)

| 메서드 | 경로 | 설명 |
|---|---|---|
| POST | `/api/setup` | 관리자 토큰 + Redis URL 저장 |

## 데이터 파일

`data/` 폴더 안:

- `config.json` — 관리자 토큰, Redis URL
- `sites.json` — 등록된 사이트 + 각 사이트 설정

## 관리자 콘솔 기능

- 사이트 추가/삭제 (도메인 입력만)
- 사이트별 설정: max_active, 대기 화면 문구 (제목/부제/순번 라벨/메타 포맷)
- Origin 검증 ON/OFF + 허용 도메인 리스트
- 실시간 통계 (입장중/대기중/최대 입장)
- 현재 큐 명단 (idle 시간) + 강제 입장/추방 버튼
- 큐 전체 비우기
- 사이트별 설치 코드 자동 생성

## 다음 단계

- [ ] 사이트별 통계 차트 (입장/대기 시계열)
- [ ] 봇 차단 (캡차)
- [ ] 와일드카드 도메인 (`*.example.com`)
- [ ] Redis Pub/Sub 으로 다중 서버 promote 조율
- [ ] WS 이벤트 기반 broadcast (지금은 2초 tick)
- [ ] 관리자 콘솔 내부에서 서버 설정(admin_token/redis_url) 수정
