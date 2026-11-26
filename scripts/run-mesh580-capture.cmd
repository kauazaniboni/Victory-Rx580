@echo off
rem RX 580 fallback with INTERNAL frame capture (not GDI/PrintWindow).
rem Usage: run-mesh580-capture.cmd "<game dir>\game_scarlett_master.exe" [hash1,hash2,...]
rem Captures powers of two plus offsets from the world's first 500-draw frame.
set "VICTORY_CONFIG=C:\Games\VItoriaBr\victory-mesh580.toml"
set "VICTORY_CACHE_DIR=C:\Games\VItoriaBr\victory-data\cache-mesh580-pre"
set "VICTORY_MESH_COMPUTE=1"
set "VICTORY_MESH_WAVE64_DAG=1"
set "VICTORY_MESH_INDIRECT=1"
set "VICTORY_SNAPSHOT_POOL_MIB=128"
set "VICTORY_D3D12_DRED=1"
set "VICTORY_CAPTURE=C:\Games\VItoriaBr\logs\capture-mesh580"
set "VICTORY_CAPTURE_WORLD=0,1,2,4,8,16,32,64,128"
set "VICTORY_LOG=info,victory_gpu::capture=info"
if not "%~2"=="" set "VICTORY_SKIP_PIPELINES=%~2"
"C:\Games\VItoriaBr\target-mesh580\diag\victory.exe" run "%~1" --execute
