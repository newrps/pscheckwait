# Pscheckwait 운영 배포 가이드 (Oracle Cloud)

Oracle Cloud Free Tier에서 `queue.zam.kr` 도메인에 HTTPS + Redis + 자동백업까지 붙인 실제 배포 기록.

도메인/IP만 본인 환경에 맞게 바꾸면 그대로 재현 가능.

---

## 목차

1. [전체 구성도](#1-전체-구성도)
2. [Oracle Cloud 인스턴스 생성](#2-oracle-cloud-인스턴스-생성)
3. [SSH 접속 설정](#3-ssh-접속-설정)
4. [서버 환경 준비](#4-서버-환경-준비)
5. [Pscheckwait 바이너리 배포](#5-pscheckwait-바이너리-배포)
6. [Redis 설치 및 연결](#6-redis-설치-및-연결)
7. [systemd 서비스 등록](#7-systemd-서비스-등록)
8. [방화벽 (iptables + Security List)](#8-방화벽-iptables--security-list)
9. [도메인 연결](#9-도메인-연결)
10. [Caddy 설치 + HTTPS](#10-caddy-설치--https)
11. [자동 백업](#11-자동-백업)
12. [동작 확인](#12-동작-확인)
13. [운영 체크리스트](#13-운영-체크리스트)

---

## 1. 전체 구성도

```
                인터넷
                  │
                  ▼ HTTPS (443)
        ┌──────────────────┐
        │  Caddy (역방향)   │ ← Let's Encrypt 자동 인증서
        └────────┬─────────┘
                 │ HTTP (3000, localhost only)
                 ▼
        ┌──────────────────┐
        │ pscheckwait-     │ ← systemd 관리
        │ server-linux     │
        └────────┬─────────┘
                 │
                 ▼
        ┌──────────────────┐
        │  Redis (6379)    │ ← 큐 상태 영속화
        └──────────────────┘

  데이터: /var/lib/pscheckwait/   (config.json, sites.json)
  백업:   /var/backups/pscheckwait/  (매일 04:00, 30일 보관)
```

**스택 요약**:
- **Cloud**: Oracle Cloud Infrastructure Free Tier
- **OS**: Ubuntu 22.04 LTS (AMD64)
- **Web Server**: Caddy 2 (자동 HTTPS)
- **App**: pscheckwait-server-linux (Rust 단일 바이너리)
- **DB**: Redis 7
- **Domain**: 호스팅케이알 DNS
- **Process Manager**: systemd

---

## 2. Oracle Cloud 인스턴스 생성

### 가입

1. https://www.oracle.com/cloud/free/ → Start for free
2. 이메일 + 결제수단 (실제 청구 없음, 한도내 사용시)
3. 홈리전 선택 — 한국이면 **Seoul** 또는 **Chuncheon** 추천

### VCN 생성 (네트워크)

대시보드 → Networking → Virtual Cloud Networks → **`Start VCN Wizard`**

> 일반 "Create VCN" 말고 **Wizard** 사용. 인터넷 게이트웨이 + 서브넷 자동 세팅됨.

- **Create VCN with Internet Connectivity** 선택
- VCN Name: `ps-vcn`
- CIDR: 기본값 (`10.0.0.0/16`)
- Next → Create

### 인스턴스 생성

Compute → Instances → **Create instance**

| 항목 | 값 |
|---|---|
| Name | `ps-vnic` |
| Image | Ubuntu 22.04 (Canonical) |
| Shape | VM.Standard.E2.1.Micro (Always Free) |
| Network | 위에서 만든 `ps-vcn`의 public subnet |
| Public IPv4 | **반드시 할당** |
| SSH keys | Generate a key pair → **둘다 다운로드** (private + public) |

> ⚠️ ARM(A1.Flex)이 안잡히면 다른 리전 시도하거나 AMD E2.1.Micro 사용. AMD도 무료.

> ⚠️ Private key (`.key` 파일)는 한번만 받을 수 있음. 안전한 곳에 보관.

생성 완료 후 인스턴스 상세에서 **Public IP** 확인 (예: `168.107.54.47`).

---

## 3. SSH 접속 설정

### Windows PowerShell

```powershell
# 키 파일 권한 (중요 - 안하면 거부됨)
icacls C:\path\to\ssh-key-xxxx.key /inheritance:r /grant:r "$env:USERNAME:(R)"

# 접속
ssh -i C:\path\to\ssh-key-xxxx.key ubuntu@168.107.54.47
```

### Xshell 쓸 때 주의

- **User Name**: `ubuntu` (Windows 사용자명 그대로 두면 거부됨)
- **Authentication**: Public Key → 받은 .key 파일 선택
- Passphrase 없으면 비워둠

---

## 4. 서버 환경 준비

접속 후 기본 패키지 업데이트:

```bash
sudo apt update
sudo apt upgrade -y
sudo apt install -y curl wget ca-certificates gnupg
```

데이터 디렉토리 만들기:

```bash
sudo mkdir -p /opt/pscheckwait
sudo mkdir -p /var/lib/pscheckwait
sudo chown -R ubuntu:ubuntu /opt/pscheckwait /var/lib/pscheckwait
```

---

## 5. Pscheckwait 바이너리 배포

### 로컬에서 Linux 바이너리 빌드 (Windows에서 Docker 사용)

```powershell
cd C:\git\waitline\server
docker run --rm -v "C:/git/waitline:/work" -w /work/server rust:1-bookworm `
  cargo build --release --target-dir target-linux

# 결과: server/target-linux/release/pscheckwait-server
```

### 서버로 업로드

```powershell
scp -i C:\path\to\ssh-key.key `
  C:\git\waitline\dist\pscheckwait-server-linux `
  ubuntu@168.107.54.47:/opt/pscheckwait/
```

### 실행 권한 + 1차 테스트

```bash
chmod +x /opt/pscheckwait/pscheckwait-server-linux
PSCHECKWAIT_NO_BROWSER=1 /opt/pscheckwait/pscheckwait-server-linux
```

로그에 `listening on http://0.0.0.0:3000` 뜨면 정상. `Ctrl+C` 종료.

---

## 6. Redis 설치 및 연결

```bash
sudo apt install -y redis-server
sudo systemctl enable --now redis-server
redis-cli ping   # PONG 떠야 정상
```

기본 설정으로 충분 (`127.0.0.1:6379`, 패스워드 없음, 로컬 전용).

> Redis는 localhost에만 바인딩되니까 외부 접근 불가. 같은 머신의 pscheckwait만 접속.

---

## 7. systemd 서비스 등록

```bash
sudo tee /etc/systemd/system/pscheckwait.service > /dev/null <<'EOF'
[Unit]
Description=Pscheckwait queue server
After=network.target redis-server.service
Requires=redis-server.service

[Service]
Type=simple
User=ubuntu
WorkingDirectory=/opt/pscheckwait
ExecStart=/opt/pscheckwait/pscheckwait-server-linux
Restart=on-failure
RestartSec=5

Environment=PSCHECKWAIT_NO_BROWSER=1
Environment=PSCHECKWAIT_DATA_DIR=/var/lib/pscheckwait
Environment=PSCHECKWAIT_PORT=3000
Environment=PSCHECKWAIT_BIND=127.0.0.1
Environment=PSCHECKWAIT_REDIS_URL=redis://127.0.0.1:6379

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable --now pscheckwait
sudo systemctl status pscheckwait
```

로그 확인:

```bash
journalctl -u pscheckwait -f
```

`Redis connected — using RedisBackend` 보이면 성공.

> **PSCHECKWAIT_BIND=127.0.0.1** 중요 — 외부 직접 접근 차단, 반드시 Caddy 거치게.

---

## 8. 방화벽 (iptables + Security List)

Oracle은 **방화벽 이중 구조**:
- **OS 방화벽**: iptables (인스턴스 내부)
- **클라우드 방화벽**: Security List (VCN 레벨)

### iptables (OS)

Oracle Ubuntu 기본 iptables는 22번만 열고 나머지 막혀있음.

```bash
sudo iptables -L INPUT -n --line-numbers
# 5번 줄에 REJECT 규칙이 있을 거임
```

REJECT 규칙 **앞**에 80/443 허용 추가:

```bash
sudo iptables -I INPUT 5 -m state --state NEW -p tcp --dport 80 -j ACCEPT
sudo iptables -I INPUT 5 -m state --state NEW -p tcp --dport 443 -j ACCEPT

# 영구 저장
sudo apt install -y netfilter-persistent iptables-persistent
sudo netfilter-persistent save
```

> 3000번 외부 노출 ❌ — Caddy가 localhost로 프록시하니까 외부에선 80/443만 필요.

### Security List (VCN)

Networking → Virtual Cloud Networks → `ps-vcn` → Public Subnet → Default Security List → **Add Ingress Rules**

| Source CIDR | Protocol | Port | 용도 |
|---|---|---|---|
| 0.0.0.0/0 | TCP | 80 | HTTP (Caddy 인증서 발급용) |
| 0.0.0.0/0 | TCP | 443 | HTTPS |
| 0.0.0.0/0 | TCP | 22 | SSH (이미 있음) |

---

## 9. 도메인 연결

호스팅케이알 (또는 다른 등록업체)에서 DNS 추가:

| Type | Name | Value | TTL |
|---|---|---|---|
| A | queue | 168.107.54.47 | 300 |

> `queue` = 서브도메인. 전체 도메인은 `queue.zam.kr`이 됨.

확인:

```bash
nslookup queue.zam.kr
# Address: 168.107.54.47
```

DNS 전파에 5분~수시간 걸릴 수 있음.

---

## 10. Caddy 설치 + HTTPS

### 설치

```bash
sudo apt install -y debian-keyring debian-archive-keyring apt-transport-https
curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' | \
  sudo gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' | \
  sudo tee /etc/apt/sources.list.d/caddy-stable.list
sudo apt update
sudo apt install -y caddy
```

### Caddyfile 작성

```bash
sudo tee /etc/caddy/Caddyfile > /dev/null <<'EOF'
queue.zam.kr {
    reverse_proxy 127.0.0.1:3000
}
EOF

sudo systemctl reload caddy
```

Caddy가 자동으로:
- Let's Encrypt 인증서 발급
- HTTP → HTTPS 리다이렉트
- WebSocket Upgrade 헤더 통과
- 인증서 자동 갱신 (60일마다)

### 확인

```bash
curl -I https://queue.zam.kr
# HTTP/2 200
```

브라우저에서 `https://queue.zam.kr/admin/` 접속하면 관리자 페이지 보임.

---

## 11. 자동 백업

### 백업 스크립트

```bash
sudo tee /usr/local/bin/pscheckwait-backup.sh > /dev/null <<'EOF'
#!/bin/bash
set -e
BACKUP_DIR="/var/backups/pscheckwait"
DATE=$(date +%Y%m%d-%H%M%S)
RETAIN_DAYS=30

mkdir -p "$BACKUP_DIR"

# 데이터 디렉토리 (config.json, sites.json)
tar czf "$BACKUP_DIR/data-$DATE.tar.gz" -C /var/lib pscheckwait/

# Redis 스냅샷
redis-cli BGSAVE > /dev/null
sleep 1
if [ -f /var/lib/redis/dump.rdb ]; then
  cp /var/lib/redis/dump.rdb "$BACKUP_DIR/redis-$DATE.rdb"
fi

# 오래된 백업 삭제
find "$BACKUP_DIR" -name "*.tar.gz" -mtime +$RETAIN_DAYS -delete
find "$BACKUP_DIR" -name "*.rdb" -mtime +$RETAIN_DAYS -delete

echo "$(date): backup complete → $BACKUP_DIR/data-$DATE.tar.gz, redis-$DATE.rdb"
EOF

sudo chmod +x /usr/local/bin/pscheckwait-backup.sh
```

### Cron 등록 (매일 04:00)

```bash
sudo tee /etc/cron.d/pscheckwait-backup > /dev/null <<'EOF'
0 4 * * * root /usr/local/bin/pscheckwait-backup.sh >> /var/log/pscheckwait-backup.log 2>&1
EOF
```

### 수동 테스트

```bash
sudo /usr/local/bin/pscheckwait-backup.sh
ls -lh /var/backups/pscheckwait/
```

---

## 12. 동작 확인

### 서비스 상태

```bash
systemctl status pscheckwait caddy redis-server
```

세개 모두 `active (running)` 이어야 함.

### 관리자 페이지

브라우저로 `https://queue.zam.kr/admin/` → 첫 실행 위저드에서 토큰 설정.

### 큐 테스트

1. 관리자에서 사이트 추가: `localhost`
2. `https://queue.zam.kr/?wait=1` 또는 데모 페이지에서 토큰 발급 시도
3. Redis에 키 들어갔는지:
   ```bash
   redis-cli KEYS 'pcw:*'
   # pcw:active:localhost, pcw:waiting:localhost 등
   ```

### 로그

```bash
# 앱 로그
journalctl -u pscheckwait -n 50

# Caddy 로그
journalctl -u caddy -n 50

# 백업 로그
tail /var/log/pscheckwait-backup.log
```

---

## 13. 운영 체크리스트

### 보안

- ☑ 3000번 포트 외부 차단 (iptables, BIND=127.0.0.1)
- ☑ Redis localhost 전용
- ☑ HTTPS (Caddy 자동)
- ☑ 관리자 토큰 안전 보관
- ☐ SSH 키 별도 보관 (분실시 인스턴스 재생성)
- ☐ Origin 검증 활성화 (`/admin/`에서 사이트별 설정)

### 모니터링 (선택)

- **Uptime Robot** (무료) — 5분 간격으로 `https://queue.zam.kr` 헬스체크
- **Telegram Bot** — systemd 실패시 알림 (`OnFailure=` 옵션 + 쉘 스크립트)

### 업데이트 절차

새 바이너리 배포할 때:

```bash
# 1. 로컬에서 새 빌드
docker run --rm -v "C:/git/waitline:/work" -w /work/server rust:1-bookworm `
  cargo build --release --target-dir target-linux

# 2. 업로드
scp -i key.key target-linux/release/pscheckwait-server \
  ubuntu@168.107.54.47:/tmp/pscheckwait-server-new

# 3. 서버에서 교체 + 재시작
sudo systemctl stop pscheckwait
sudo mv /tmp/pscheckwait-server-new /opt/pscheckwait/pscheckwait-server-linux
sudo chmod +x /opt/pscheckwait/pscheckwait-server-linux
sudo systemctl start pscheckwait
sudo systemctl status pscheckwait
```

다운타임은 1초 미만.

### 복구

데이터 손실시:

```bash
sudo systemctl stop pscheckwait

# config + sites 복구
sudo tar xzf /var/backups/pscheckwait/data-YYYYMMDD-HHMMSS.tar.gz -C /var/lib/

# Redis 복구 (필요시)
sudo systemctl stop redis-server
sudo cp /var/backups/pscheckwait/redis-YYYYMMDD-HHMMSS.rdb /var/lib/redis/dump.rdb
sudo chown redis:redis /var/lib/redis/dump.rdb
sudo systemctl start redis-server

sudo systemctl start pscheckwait
```

---

## 부록: 환경변수 레퍼런스

| 변수 | 기본값 | 설명 |
|---|---|---|
| `PSCHECKWAIT_PORT` | 3000 | 리스닝 포트 |
| `PSCHECKWAIT_BIND` | 0.0.0.0 | 바인딩 주소 (운영: 127.0.0.1) |
| `PSCHECKWAIT_DATA_DIR` | `./data` | config/sites 저장 경로 |
| `PSCHECKWAIT_REDIS_URL` | (없음) | 설정시 Redis 사용, 없으면 인메모리 |
| `PSCHECKWAIT_NO_BROWSER` | 0 | 1로 설정시 브라우저 자동 안열림 |
| `PSCHECKWAIT_ADMIN_TOKEN` | (config.json) | 환경변수로 토큰 강제 지정 가능 |

---

## 부록: 비용

Oracle Cloud Free Tier 사용시 **이론적으로 영구 무료**:

| 리소스 | Always Free 한도 | 본 배포 사용량 |
|---|---|---|
| VM.Standard.E2.1.Micro | 2대 | 1대 |
| Block Storage | 200GB | ~20GB |
| Outbound 트래픽 | 10TB/월 | 트래픽 따라 |
| VCN | 무제한 | 1개 |

도메인 비용 (zam.kr 등) 별도 — 호스팅케이알에서 연간 결제.

---

## 관련 문서

- **일반 사용법**: `GUIDE.md`
- **API 명세**: `README.md`
- **서버 코드**: `server/src/main.rs`, `server/src/backend.rs`
- **로더 코드**: `loader/pscheckwait.js`
- **관리자 SPA**: `admin-sveltekit/`
