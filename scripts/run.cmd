@echo off
rem Usage: run.cmd "C:\Games\VItoriaBr\victory-data\game\<title_exe>"
if "%~1"=="" (echo Usage: run.cmd ^<path to the title executable in your dump^> & exit /b 1)
set "VICTORY_CONFIG=C:\Games\VItoriaBr\victory.toml"
rem RX 580: let the card check ~1 MiB shader-write copies instead of the CPU.
rem Measured: removes ~350 MB/frame of CPU compares and 228 log lines/frame.
set "VICTORY_CARD_BUFFERS_MIN_MB=1"
"C:\Games\VItoriaBr\target\release\victory.exe" run "%~1" --execute
