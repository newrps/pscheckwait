# Pscheckwait 사용 가이드

설치부터 실제 사이트에 대기열 붙이기까지 전 과정.

> **운영 서버 배포 가이드 따로 있음** → [DEPLOY.md](DEPLOY.md)
> (Oracle Cloud + Caddy + Redis + 자동백업 실전 셋업)

---

## 목차

1. [설치](#1-설치)
2. [첫 실행 (관리자 토큰 설정)](#2-첫-실행-관리자-토큰-설정)
3. [사이트 등록](#3-사이트-등록)
4. [고객 사이트에 스크립트 박기](#4-고객-사이트에-스크립트-박기)
5. [대기 화면 문구 수정](#5-대기-화면-문구-수정)
6. [입장 인원 조절](#6-입장-인원-조절)
7. [큐 수동 관리 (강제 입장/추방)](#7-큐-수동-관리-강제-입장추방)
8. [Origin 검증으로 도용 막기](#8-origin-검증으로-도용-막기)
9. [Redis로 큐 영속화](#9-redis로-큐-영속화)
10. [백업/이전](#10-백업이전)
11. [트러블슈팅](#11-트러블슈팅)

---

## 1. 설치

### 방법 A: Windows 실행파일

1. `dist/pscheckwait-server.exe` 다운로드
2. 원하는 폴더에 둠 (예: `C:\pscheckwait\`)
3. 더블클릭 또는 터미널에서 실행

```powershell
cd C:\pscheckwait
.\pscheckwait-server.exe
```

콘솔에 다음과 같이 뜨면 성공:

```
INFO pscheckwait_server: data directory: C:\pscheckwait\data
INFO pscheckwait_server: using in-memory backend
INFO pscheckwait_server: pscheckwait server listening on http://localhost:3000
WARN pscheckwait_server: FIRST RUN: open http://localhost:3000 to set admin token
```

브라우저가 자동으로 `http://localhost:3000` 열어줌.

### 방법 B: Linux 서버 (x86_64)

1. `dist/pscheckwait-server-linux` 서버에 업로드
2. 실행 권한 부여 후 실행

```bash
chmod +x pscheckwait-server-linux
./pscheckwait-server-linux
```

서버에 브라우저 없으니까 자동 오픈은 건너뛰어짐 (xdg-open 없으면 무해하게 실패).

**원격 접속 시 주의**: 기본은 `0.0.0.0:3000`으로 바인딩되므로 방화벽에서 3000번 열어야 함:

```bash
sudo ufw allow 3000/tcp
```

운영 환경이면 nginx/caddy로 리버스 프록시 + HTTPS 권장 (자세한 건 아래 "운영 배포" 섹션).

**systemd 서비스로 등록**:

```ini
# /etc/systemd/system/pscheckwait.service
[Unit]
Description=Pscheckwait queue server
After=network.target

[Service]
Type=simple
User=pscheckwait
WorkingDirectory=/opt/pscheckwait
ExecStart=/opt/pscheckwait/pscheckwait-server-linux
Restart=on-failure
Environment=PSCHECKWAIT_NO_BROWSER=1

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now pscheckwait
sudo systemctl status pscheckwait
```

### 방법 C: macOS

같은 방식으로 빌드한 macOS 바이너리는 제공 안되지만, 소스에서 빌드 가능 (방법 D 참고).

### 방법 D: 소스에서 빌드 (개발자용)

Rust 1.83+ 필요 (의존성중 edition2024 필요):

**Windows / macOS / Linux 공통**:

```bash
git clone <repo>
cd pscheckwait/server
cargo build --release
# 결과: target/release/pscheckwait-server[.exe]
```

**Docker로 Linux 바이너리 만들기** (Windows에서):

```powershell
docker run --rm -v "C:/git/waitline:/work" -w /work/server rust:1-bookworm `
  cargo build --release --target-dir target-linux
# 결과: server/target-linux/release/pscheckwait-server
```

---

## 2. 첫 실행 (관리자 토큰 설정)

처음 실행하면 브라우저에서 **초기 설정 화면**이 뜸:

```
┌──────────────────────────────────────────┐
│ Pscheckwait                                 │
│ 처음 오신 것을 환영합니다 👋             │
│                                          │
│ 관리자 토큰 *                            │
│ [____________________] [생성]            │
│                                          │
│ □ Redis 백엔드 사용 (선택)               │
│                                          │
│         [   시작하기   ]                 │
└──────────────────────────────────────────┘
```

**할 일**:

1. **[생성]** 버튼 클릭 → 랜덤 토큰 자동 입력
2. 토큰을 **메모장에 복사 저장** (잊으면 초기화해야 함)
3. Redis는 일단 끄고 진행 (나중에 켜도 됨)
4. **[시작하기]** 클릭

토큰이 저장되고 관리자 콘솔로 이동됨.

### 토큰 잊었을 때

`data/config.json` 파일 삭제 후 서버 재시작 → 다시 첫 실행 모드.

```powershell
Remove-Item C:\pscheckwait\data\config.json
.\pscheckwait-server.exe
```

---

## 3. 사이트 등록

관리자 콘솔 (`http://localhost:3000/admin/`)에서:

```
┌──────────────┬──────────────────────────────┐
│ Pscheckwait     │ 📋 왼쪽에서 사이트를          │
│ ─────────    │    선택하거나 추가하세요      │
│ 사이트       │                              │
│ ● localhost  │                              │
│              │                              │
│ ─── 추가 ─── │                              │
│ [example.com]│                              │
│ [   추가  ]  │                              │
└──────────────┴──────────────────────────────┘
```

**할 일**:

1. 왼쪽 **"새 사이트 추가"** 입력란에 도메인 입력 (`example.com`)
2. **[추가]** 클릭
3. 좌측 목록에서 해당 도메인 클릭

> **도메인 입력 규칙**: 프로토콜 빼고 호스트만 (`example.com`, `shop.example.com`). 포트는 무시. 서브도메인은 별개 사이트로 취급.

### 자동 등록된 `localhost`

데모/테스트용으로 `localhost`가 자동 등록돼있음. 로컬 개발에선 별도 등록 불필요.

---

## 4. 고객 사이트에 스크립트 박기

사이트 선택하면 우측 하단에 **설치 코드** 패널이 보임:

```
설치 코드
─────────────────────────────────────────
<script src="http://localhost:3000/loader/pscheckwait.js" async></script>
```

**할 일**:

1. 이 한 줄을 복사
2. 고객 사이트의 **`<head>` 안 어디든** 붙여넣기
3. 사이트 새로고침

운영 환경이면 `localhost:3000` 부분을 실제 Pscheckwait 서버 주소로 바꿔야 함:

```html
<script src="https://queue.your-domain.com/loader/pscheckwait.js" async></script>
```

### 동작 확인

- 한도 이내(`max_active` 미만) 접속자: 페이지 정상 표시
- 한도 초과 접속자: 대기 화면 표시

대기 화면 우측 하단에 `ws`(WebSocket) 또는 `polling` 표시됨 — 전송 방식 확인용.

---

## 5. 대기 화면 문구 수정

관리자에서 사이트 선택 → **설정** 패널:

| 필드 | 설명 | 예시 |
|---|---|---|
| 대기 화면 제목 | 큰 제목 | `잠시만 기다려 주세요` |
| 대기 화면 부제 | 부연 설명 | `현재 접속자가 많습니다` |
| 순번 라벨 | 큰 숫자 아래 텍스트 | `번째 대기 중` |
| 메타 정보 포맷 | 하단 정보 줄 | `대기 {waiting}명 · 입장 {active}명` |

**치환자** (메타 포맷에서만):

- `{waiting}` — 전체 대기자 수
- `{active}` — 현재 입장중인 사람 수
- `{position}` — 본인 순번

**할 일**:

1. 입력란 수정
2. **[저장]** 클릭
3. 새로 들어오는 대기자부터 바뀐 문구 표시
4. **[미리보기]** 버튼으로 `/demo/` 페이지 열어 확인

---

## 6. 입장 인원 조절

**최대 동시 입장 인원** 필드 = `max_active`.

| 상황 | 권장 값 |
|---|---|
| 일반 운영 | 1,000~10,000 (서버 처리량에 맞춰) |
| 한정판 드랍/티켓팅 | 100~500 (좁게 줘서 큐 길이 유지) |
| 테스트 | 1~3 (탭 몇개로 대기 화면 확인) |

**주의사항**:
- 줄여도 **이미 입장한 사용자는 안 쫓겨남**. 새 입장만 제한됨.
- 입장 세션은 `heartbeat` 끊긴 후 60초에 자동 만료.

---

## 7. 큐 수동 관리 (강제 입장/추방)

관리자에서 사이트 선택 → **큐 관리** 패널:

```
큐 관리                            [전부 비우기]
─────────────────────────────────────────────
입장중 (Active)
  ● a3b2c8e4…   2분 12초 전        [추방]
  ● 7f1d094b…   45초 전            [추방]

대기중 (Waiting)
  #1  c8e4a3b2…   30초 전  [입장]  [추방]
  #2  b9f8e7d6…   25초 전  [입장]  [추방]
  #3  a1b2c3d4…   18초 전  [입장]  [추방]
```

**할 수 있는 액션**:

- **[입장]** — 대기자를 즉시 active로 승급 (max_active 무시함)
- **[추방]** — 토큰을 큐와 active 양쪽에서 제거. 해당 유저는 새로고침시 새 토큰 받아 큐 뒤로 감
- **[전부 비우기]** — 사이트의 모든 토큰 제거 (active + waiting 둘다)

**사용 예시**:
- VIP를 줄 안 세우고 바로 입장시키기 → **[입장]**
- 봇 의심되는 토큰 차단 → **[추방]** (계속 들어오면 봇차단 필요, 미구현)
- 큐 리셋하고 새로 받기 → **[전부 비우기]**

---

## 8. Origin 검증으로 도용 막기

기본적으로 **켜져 있음**. 사이트 도메인과 다른 Origin에서 오는 요청 차단.

### 기본 동작 (`허용 도메인` 비워둠)

`example.com` 사이트라면:
- ✅ `https://example.com`에서 호출 → 통과
- ❌ `https://evil.com`에서 호출 → 403 차단
- ❌ Origin 헤더 없는 요청 → 403

### 여러 도메인 허용

`허용 도메인` 필드에 콤마로 구분 입력:

```
example.com, www.example.com, m.example.com
```

이렇게 하면 위 세개에서만 허용. **사이트 도메인 자체도 명시해야 함** (자동 포함 안됨).

### Origin 검증 끄기 (비권장)

`Origin 검증 사용` 체크박스 해제. 누구나 큐에 접근 가능 → 도용/스팸 위험.

> 운영 환경에선 절대 끄지 말 것. 테스트/디버깅용으로만 일시적 사용.

---

## 9. Redis로 큐 영속화

**언제 필요?**
- 서버 재시작에도 큐를 유지하고 싶을 때
- 여러 서버 인스턴스 운영할 때

**기본은 인메모리** — 서버 재시작하면 큐 비워짐. 사이트 설정(`sites.json`)은 어차피 디스크 저장이라 안 사라짐.

### Redis 설치

가장 빠른 방법: Docker

```powershell
docker run -d --name pscheckwait-redis -p 6379:6379 redis:7-alpine
```

또는 Windows용 [Memurai](https://www.memurai.com/) 설치.

### Pscheckwait에 연결

**옵션 1: 환경변수**

```powershell
$env:PSCHECKWAIT_REDIS_URL = "redis://127.0.0.1:6379"
.\pscheckwait-server.exe
```

**옵션 2: 설치 위저드에서** (첫 실행시)

위저드에서 `Redis 백엔드 사용` 체크 → URL 입력 → 저장 → 서버 재시작

**옵션 3: config.json 직접 편집**

```json
{
  "admin_token": "your-token",
  "redis_url": "redis://127.0.0.1:6379"
}
```

서버 재시작 → 콘솔에 `Redis connected — using RedisBackend` 보이면 성공.

### Redis 연결 실패시

서버가 자동으로 인메모리로 폴백. 콘솔 로그에 에러 표시.

---

## 10. 백업/이전

**필요한 것은 `data/` 폴더만**:

```
data/
├── config.json     관리자 토큰, Redis URL
└── sites.json      등록된 사이트 + 설정
```

### 다른 PC로 이전

1. `pscheckwait-server.exe` 복사
2. `data/` 폴더 복사 (exe 옆에)
3. 새 PC에서 실행 → 같은 설정 그대로

### 백업

```powershell
Compress-Archive -Path C:\pscheckwait\data -DestinationPath pscheckwait-backup.zip
```

Redis 사용중이면 큐 상태는 Redis 안에 있음 → Redis도 별도 백업 필요.

---

## 폐쇄망(에어갭) 배포

Pscheckwait 바이너리는 외부 의존성 0이라서 **인터넷 없는 환경에서 그대로 동작**합니다. 텔레메트리/자동업데이트/CDN/외부 API 호출 전혀 없음.

### 빌드 vs 런타임

| 단계 | 인터넷 필요? |
|---|---|
| 빌드 (`cargo build`) | ✅ crates.io에서 의존성 다운로드 |
| 실행 | ❌ 불필요 |

→ **인터넷 PC에서 빌드 → 폐쇄망 서버로 바이너리 복사** 워크플로.

### 배포 절차

**1. 인터넷 PC에서 빌드** (한번만)

```powershell
# Windows
cd server
cargo build --release

# Linux 타겟이면 Docker 사용
docker run --rm -v "C:/git/waitline:/work" -w /work/server rust:1-bookworm `
  cargo build --release --target-dir target-linux
```

**2. 산출물을 USB/파일전송 도구로 복사**

필요한 파일은 단 하나:

- Windows: `target/release/pscheckwait-server.exe`
- Linux: `target-linux/release/pscheckwait-server`

이게 전부. 추가 라이브러리/런타임 설치 불필요.

**3. 폐쇄망 서버에 두고 실행**

```bash
chmod +x pscheckwait-server
./pscheckwait-server
```

첫 실행시 같은 위저드 동작. 외부 통신 없음.

### Redis도 같이 (선택)

Redis 컨테이너 이미지도 미리 받아놓기:

```bash
# 인터넷 PC에서
docker pull redis:7-alpine
docker save redis:7-alpine -o redis-7-alpine.tar

# tar 파일을 폐쇄망 서버로 옮긴 뒤
docker load -i redis-7-alpine.tar
docker run -d --name pscheckwait-redis -p 6379:6379 redis:7-alpine
```

또는 폐쇄망 서버가 Linux면 패키지로 직접 설치:

```bash
sudo apt install redis-server     # Ubuntu/Debian (이미 ISO에 있으면)
sudo systemctl enable --now redis-server
```

### 폐쇄망 운영시 체크리스트

- ☐ **시스템 시간**: TTL 계산이 시스템 시계 기반이므로 NTP 또는 내부 시간서버 동기화
- ☐ **DNS**: 사이트 식별자가 도메인이므로 내부 DNS가 정확해야함 (혹은 hosts 파일)
- ☐ **HTTPS 인증서**: 내부 CA로 발급받거나 자체서명. 자체서명이면 클라이언트가 신뢰해야됨
- ☐ **방화벽**: 외부망 차단된 상태에서 내부망 클라이언트→서버 3000 (또는 443) 통신만 허용
- ☐ **로그 수집**: stdout 로그를 외부 로그 수집기 안 쓰면 systemd journal 또는 파일 리다이렉트

### 검증: 진짜 인터넷 없이도 되나?

폐쇄망 환경 시뮬레이션 (Docker로):

```bash
# 인터넷 차단된 네트워크에 컨테이너 실행
docker run --rm --network none \
  -v /path/to/binary:/app \
  -p 3000:3000 \
  debian:slim /app/pscheckwait-server-linux
```

`--network none`이면 외부 통신 완전 차단. 그래도 정상 동작하면 폐쇄망 OK.

---

## 운영 배포 (Linux)

집에서 테스트할 땐 그냥 실행파일 띄우면 끝이지만, 외부 서비스로 운영하려면:

### 리버스 프록시 (nginx 예시)

브라우저는 HTTPS여야 하고 WebSocket도 통과시켜야 함:

```nginx
server {
    listen 443 ssl http2;
    server_name queue.your-domain.com;

    ssl_certificate     /etc/letsencrypt/live/queue.your-domain.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/queue.your-domain.com/privkey.pem;

    location / {
        proxy_pass http://127.0.0.1:3000;
        proxy_http_version 1.1;

        # WebSocket 통과
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";

        # 원 클라이언트 정보
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        # WS 장기 연결 타임아웃
        proxy_read_timeout 3600s;
    }
}
```

### Caddy (더 간단)

```caddy
queue.your-domain.com {
    reverse_proxy 127.0.0.1:3000
}
```

Caddy는 자동 HTTPS + 자동 WebSocket 통과.

### 데이터 디렉토리 분리

운영시엔 데이터 경로 명시 권장:

```bash
PSCHECKWAIT_DATA_DIR=/var/lib/pscheckwait ./pscheckwait-server-linux
```

`/var/lib/pscheckwait/`에 `config.json`, `sites.json` 저장됨.

### systemd + Redis 같이 띄우는 예시

```ini
# /etc/systemd/system/pscheckwait.service
[Unit]
Description=Pscheckwait queue server
After=network.target redis.service
Requires=redis.service

[Service]
Type=simple
User=pscheckwait
WorkingDirectory=/opt/pscheckwait
ExecStart=/opt/pscheckwait/pscheckwait-server-linux
Restart=on-failure

Environment=PSCHECKWAIT_NO_BROWSER=1
Environment=PSCHECKWAIT_DATA_DIR=/var/lib/pscheckwait
Environment=PSCHECKWAIT_REDIS_URL=redis://127.0.0.1:6379

[Install]
WantedBy=multi-user.target
```

### 방화벽

서버 외부 노출은 nginx/caddy의 443만:

```bash
sudo ufw allow 443/tcp
# 3000은 외부에 열지 마세요 (localhost only)
```

---

## 11. 트러블슈팅

### 포트 3000 이미 쓰는중

다른 포트로:

```powershell
$env:PSCHECKWAIT_PORT = "4000"
.\pscheckwait-server.exe
```

### 브라우저 자동 안 열림 / 안 열고 싶음

```powershell
$env:PSCHECKWAIT_NO_BROWSER = "1"
.\pscheckwait-server.exe
```

수동으로 `http://localhost:3000` 열기.

### 데모는 되는데 진짜 사이트에서 안됨

체크리스트:
1. 사이트 도메인이 관리자에 **등록**돼있나? (등록 안된 도메인은 로더가 조용히 종료)
2. **Origin 검증** 설정에 그 도메인 포함됐나?
3. `<script>` 태그가 `<head>` 안에 있나?
4. 개발자도구 콘솔에 에러 있나? (CORS, 404 등)
5. Pscheckwait 서버 URL이 **HTTPS**여야 할 수도 — 운영 사이트가 HTTPS면 로더도 HTTPS여야 함

### 큐가 영구히 차있음 (사용자 다 나갔는데)

원인: heartbeat가 안 와서 TTL(60초) 대기중. 그냥 60초 기다리거나 **[전부 비우기]** 클릭.

WebSocket이면 보통 즉시 처리됨 — close 프레임으로 leave 호출. polling이면 sendBeacon 의존.

### 사이트 추가했는데 로더가 작동 안함

도메인 정확히 일치하는지 확인:
- `example.com` 등록 → `www.example.com`에선 작동 안함
- 둘 다 쓰려면 별도 등록하거나 `허용 도메인`에 둘 다 추가

### 콘솔에 `Redis connect failed` 떴음

Redis가 안 떠있거나 URL 틀림. 일단 인메모리로 폴백돼서 동작은 함.

확인:
```powershell
docker ps | findstr redis    # 컨테이너 떠있는지
docker logs pscheckwait-redis   # Redis 로그
```

---

## 추가 자료

- **API 명세**: `README.md` 참고
- **소스 코드**: `server/src/main.rs`, `server/src/backend.rs`
- **로더 코드**: `loader/pscheckwait.js`
- **관리자 페이지**: `admin/index.html`, `admin/setup.html`
