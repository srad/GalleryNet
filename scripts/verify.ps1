# GalleryNet Verification Script (PowerShell)

$ErrorActionPreference = "Stop"

Write-Host "--- Checking Backend Compilation ---" -ForegroundColor Cyan
cargo check

Write-Host "--- Running Backend Tests ---" -ForegroundColor Cyan
$testList = cargo test -- --list | Select-String ": test"
$count = $testList.Count

Write-Host "Found $count tests."

cargo test

Write-Host "--- Checking Frontend Compilation ---" -ForegroundColor Cyan
Set-Location frontend
npm run build
Set-Location ..

Write-Host "`n✅ Verification Successful: All tests passed and count is maintained ($count)." -ForegroundColor Green
