@echo off
rem Pre-translate shaders for the mangrove + mesh fallback build.
set "VICTORY_CONFIG=C:\Games\VItoriaBr\victory-mangrove.toml"
set "VICTORY_LOG=info,victory_gpu::descriptor::host=error"
set "VICTORY_TEXTURE_GUARD=0"
set "VICTORY_SHADER_OPT=0"
set "VICTORY_TEXTURE_ROOM_PERCENT=94"
set "VICTORY_LATE_WAIT=2"
set "VICTORY_PIPELINE_WAIT_MS=24"
set "VICTORY_POOL_NO_ASSERT=1"
set "VICTORY_BVH_HOLD=1"
set "VICTORY_SKIP_PIPELINES=00167d58cc5a78d7"
set "VICTORY_MESH_COMPUTE=1"
set "VICTORY_MESH_WAVE64_DAG=1"
set "VICTORY_MESH_INDIRECT=1"
set "VICTORY_SNAPSHOT_POOL_MIB=128"
"C:\Games\VItoriaBr\target-mangrove\diag\victory.exe" warm "C:\Games\VItoriaBr\victory-data\game\shaders.rpf" "C:\Games\VItoriaBr\victory-data\game\game_scarlett_master.exe"
echo MANGROVE_WARM_EXIT=%ERRORLEVEL%
