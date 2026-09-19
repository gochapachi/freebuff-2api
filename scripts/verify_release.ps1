# verify_release.ps1 — Freebuff2API v0.10 发布前本地验证（失败即非 0 退出）
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$cargo = "$env:USERPROFILE\.cargo\bin\cargo.exe"
if (!(Test-Path $cargo)) { $cargo = 'cargo' }
Write-Host "==> cargo fmt --check" -ForegroundColor Cyan
& $cargo fmt --check; if ($LASTEXITCODE -ne 0) { throw "fmt 未通过，先运行 cargo fmt" }
Write-Host "==> clippy" -ForegroundColor Cyan
& $cargo clippy --all-targets -- -D warnings; if ($LASTEXITCODE -ne 0) { throw "clippy 未通过" }
Write-Host "==> cargo test --lib" -ForegroundColor Cyan
& $cargo test --lib; if ($LASTEXITCODE -ne 0) { throw "lib 单测未通过" }
Write-Host "==> 集成测试" -ForegroundColor Cyan
& $cargo test --test core_test; if ($LASTEXITCODE -ne 0) { throw "core_test 未通过" }
& $cargo test --test router_test; if ($LASTEXITCODE -ne 0) { throw "router_test 未通过" }
& $cargo test --test web_pool_test; if ($LASTEXITCODE -ne 0) { throw "web_pool_test 未通过" }
& $cargo test --test model_meta_test; if ($LASTEXITCODE -ne 0) { throw "model_meta_test 未通过" }
Write-Host "==> 面板 JS 语法" -ForegroundColor Cyan
node tests/check_panel_js.cjs; if ($LASTEXITCODE -ne 0) { throw "面板 JS 语法未通过" }
Write-Host "==> 模型合同对齐" -ForegroundColor Cyan
node scripts/check_model_contract.mjs; if ($LASTEXITCODE -ne 0) { throw "模型合同漂移" }
Write-Host "==> release 冒烟（端口 47990）" -ForegroundColor Cyan
$tmp = Join-Path $env:TEMP "fba-release-smoke"
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
$cfg = '{"listen_addr":"127.0.0.1:47990","upstream_base_url":"https://www.codebuff.com","auth_tokens":[],"api_keys":[],"skip_upstream_check":true,"memory_enabled":false,"sqlite_path":"data/freebuff2api.sqlite","telemetry_path":"data/telemetry.sqlite","memory_path":"data/memory.sqlite","skills_dir":"data/skills","redact_logs":true,"token_saver":false}'
Set-Content -LiteralPath "$tmp\config.json" -Value $cfg -Encoding ASCII
if (!(Test-Path "$root\target\release\freebuff2api.exe")) { throw "缺少 release 二进制，请先 cargo build --release" }
$proc = Start-Process -FilePath "$root\target\release\freebuff2api.exe" -ArgumentList "--config","config.json" -WorkingDirectory $tmp -PassThru -WindowStyle Hidden
try {
  $ok = $false
  for ($i = 0; $i -lt 60; $i++) { Start-Sleep -Milliseconds 500; try { $r = Invoke-WebRequest "http://127.0.0.1:47990/healthz" -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {} }
  if (-not $ok) { throw "网关 60 秒内未就绪" }
  $ui = Invoke-WebRequest "http://127.0.0.1:47990/ui" -UseBasicParsing -TimeoutSec 5
  if ($ui.StatusCode -ne 200) { throw "/ui 非 200" }
  $models = Invoke-WebRequest "http://127.0.0.1:47990/v1/models" -UseBasicParsing -TimeoutSec 5
  if ($models.StatusCode -ne 200) { throw "/v1/models 非 200" }
  Write-Host "冒烟通过：healthz/ui/models 200" -ForegroundColor Green
} finally {
  if ($proc -and -not $proc.HasExited) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
}
Write-Host "全部本地验证通过 ✅" -ForegroundColor Green