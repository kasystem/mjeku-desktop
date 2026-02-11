@echo off
setlocal

cd /d "%~dp0.."

rem Run Tauri dev in background-friendly mode and capture logs for debugging.
call scripts\\run_tauri_dev_win.cmd > tauri-dev-live.log 2>&1

endlocal

