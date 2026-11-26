@echo off
rem Experimental mesh -> compute fallback build for GPUs without a mesh tier
rem (RX 580 / Polaris). Based on PR #2203, branch codex/gtx-upstream-update.
set "VICTORY_CONFIG=C:\Games\VItoriaBr\victory-mesh580.toml"
set "VICTORY_MESH_COMPUTE=1"
set "VICTORY_MESH_WAVE64_DAG=1"
set "VICTORY_MESH_INDIRECT=1"
set "VICTORY_SNAPSHOT_POOL_MIB=128"
set "VICTORY_D3D12_DRED=1"
"C:\Games\VItoriaBr\target-mesh580\diag\victory.exe" run "%~1" --execute
