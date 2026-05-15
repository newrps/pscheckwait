# Pscheckwait deploy script
# Usage:
#   .\deploy.ps1                    # 빌드 + 배포
#   .\deploy.ps1 -NoBuild           # 기존 바이너리로 배포만
#   .\deploy.ps1 -NoGit             # git commit/push 건너뜀
#   .\deploy.ps1 -Message "fix bug" # git 커밋 메시지 지정
#   .\deploy.ps1 -DryRun            # 실제 실행 없이 계획만 출력

param(
  [string]$Message = "",
  [switch]$NoBuild,
  [switch]$NoGit,
  [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot

# ---- 설정 ----
# SSH는 ~/.ssh/config 의 "Host pscheckwait" alias 사용
$SSHHost    = "pscheckwait"
$RemotePath = "/opt/pscheckwait/pscheckwait-server-linux"
$Binary     = "$root\server\target-linux\release\pscheckwait-server"
$HealthUrl  = "https://queue.zam.kr/api/health"

function Step($msg) { Write-Host ""; Write-Host "==> $msg" -ForegroundColor Cyan }
function Skip($msg) { Write-Host "    [skip] $msg" -ForegroundColor DarkGray }
function Run($cmd) {
  if ($DryRun) { Write-Host "    [dry] $cmd" -ForegroundColor Yellow; return }
  Write-Host "    $cmd" -ForegroundColor DarkGray
  Invoke-Expression $cmd
  if ($LASTEXITCODE -ne 0) { throw "command failed: $cmd" }
}

# ---- 1. Git ----
Step "Git"
if ($NoGit) {
  Skip "git commit/push (-NoGit)"
} else {
  Set-Location $root
  $changes = git status --short
  if (-not $changes) {
    Skip "변경사항 없음"
  } else {
    Write-Host $changes
    $msg = if ($Message) { $Message } else {
      $ts = Get-Date -Format "yyyy-MM-dd HH:mm"
      "Deploy at $ts"
    }
    Run "git add -A"
    if ($DryRun) {
      Write-Host "    [dry] git commit -m `"$msg`""
      Write-Host "    [dry] git push"
    } else {
      git commit -m $msg
      if ($LASTEXITCODE -ne 0) { throw "git commit failed" }
      git push
      if ($LASTEXITCODE -ne 0) { throw "git push failed" }
    }
  }
}

# ---- 2. Build ----
Step "Build (Linux binary via Docker)"
if ($NoBuild) {
  Skip "cargo build (-NoBuild)"
  if (-not (Test-Path $Binary)) {
    throw "바이너리 없음: $Binary — 먼저 빌드 필요"
  }
} else {
  Run "docker run --rm -v `"${root}:/work`" -w /work/server rust:1-bookworm cargo build --release --target-dir target-linux"
}

if (-not $DryRun) {
  $size = (Get-Item $Binary).Length / 1MB
  Write-Host ("    바이너리: {0} ({1:N1} MB)" -f $Binary, $size)
}

# ---- 3. Upload ----
Step "Upload → $SSHHost"
Run "scp `"$Binary`" `"${SSHHost}:/tmp/pscheckwait-server-new`""

# ---- 4. Restart ----
Step "Restart pscheckwait service"
$remoteCmd = "sudo systemctl stop pscheckwait && sudo mv /tmp/pscheckwait-server-new $RemotePath && sudo chmod +x $RemotePath && sudo systemctl start pscheckwait && sleep 2 && sudo systemctl is-active pscheckwait"
Run "ssh $SSHHost `"$remoteCmd`""

# ---- 5. Health check ----
Step "Health check"
if ($DryRun) {
  Write-Host "    [dry] GET $HealthUrl" -ForegroundColor Yellow
} else {
  try {
    $r = Invoke-WebRequest $HealthUrl -UseBasicParsing -TimeoutSec 10
    Write-Host "    HTTP $($r.StatusCode) — $($r.Content)" -ForegroundColor Green
  } catch {
    Write-Host "    헬스체크 실패: $_" -ForegroundColor Red
    throw
  }
}

Write-Host ""
Write-Host "✓ Done." -ForegroundColor Green
