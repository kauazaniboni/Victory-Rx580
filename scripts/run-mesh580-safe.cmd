@echo off
rem RX 580 mesh->compute fallback after prebuild-mesh580.cmd.
rem Usage: run-mesh580-safe.cmd "<game dir>\game_scarlett_master.exe" [hash1,hash2,...]
rem Hashes are optional VICTORY_SKIP_PIPELINES entries for pipelines that
rem crashed the AMD compiler and were removed from the pre-build.
set "VICTORY_CONFIG=C:\Games\VItoriaBr\victory-mesh580.toml"
set "VICTORY_CACHE_DIR=C:\Games\VItoriaBr\victory-data\cache-mesh580-pre"
set "VICTORY_MESH_COMPUTE=1"
set "VICTORY_MESH_WAVE64_DAG=1"
set "VICTORY_MESH_INDIRECT=1"
set "VICTORY_SNAPSHOT_POOL_MIB=128"
set "VICTORY_D3D12_DRED=1"
if not "%~2"=="" set "VICTORY_SKIP_PIPELINES=%~2"
"C:\Games\VItoriaBr\target-mesh580\diag\victory.exe" run "%~1" --execute
