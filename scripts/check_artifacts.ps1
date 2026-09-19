# check_artifacts.ps1 — 扫描旧产物/测试残留（只报告不删除；删除需人工确认）
$p = "C:\Users\Administrator.DESKTOP-EGNE9ND\Desktop\freebuff-2api\成品\Freebuff-2API"
Write-Host "== 旧安装包（desktop/dist） ==" -ForegroundColor Cyan
Get-ChildItem "$p\desktop\dist" -Filter *.exe -ErrorAction SilentlyContinue | Where-Object { $_.Name -notmatch '0.10.0' } | Select-Object Name,@{n='MB';e={[math]::Round($_.Length/1MB,1)}} | Format-Table -AutoSize
Write-Host "== win-unpacked（本地调试产物） ==" -ForegroundColor Cyan
if (Test-Path "$p\desktop\win-unpacked") { $sz = (Get-ChildItem "$p\desktop\win-unpacked" -Recurse -File -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum; "win-unpacked 共 {0} MB" -f [math]::Round($sz/1MB,1) }
Write-Host "== data/ 测试残留库 ==" -ForegroundColor Cyan
Get-ChildItem "$p\data" -File -ErrorAction SilentlyContinue | Where-Object { $_.Name -match 'mock-demo|rev_|freebuff2api2|e2e' } | Select-Object Name,@{n='KB';e={[math]::Round($_.Length/1KB,1)}} | Format-Table -AutoSize
Write-Host "== 宣传视频大文件（>1.5MB） ==" -ForegroundColor Cyan
Get-ChildItem "$p\宣传视频" -Recurse -File -ErrorAction SilentlyContinue | Where-Object { $_.Extension -match 'mp4|png|jpg' -and $_.Length -gt 1.5MB } | Select-Object Name,@{n='MB';e={[math]::Round($_.Length/1MB,1)}} | Format-Table -AutoSize
Write-Host "== 说明 =="; Write-Host "以上仅为清单（安全只读）。删除请人工确认后操作，或将文件移到 _archive_ 观察。" -ForegroundColor Yellow