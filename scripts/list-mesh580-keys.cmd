@echo off
rem Lists every pre-build pipeline key with its shaders/state; makes no pipelines.
rem Used to map a crashed pipeline key to its 16-digit shader hashes.
set "VICTORY_CONFIG=C:\Games\VItoriaBr\victory-mesh580.toml"
set "VICTORY_CACHE_DIR=C:\Games\VItoriaBr\victory-data\cache-mesh580-pre"
set "VICTORY_LOG=info"
set "VICTORY_MESH_COMPUTE=1"
set "VICTORY_MESH_WAVE64_DAG=1"
set "VICTORY_MESH_INDIRECT=1"
set "VICTORY_SNAPSHOT_POOL_MIB=128"
set "VICTORY_PREBUILD_KEYS=C:\Games\VItoriaBr\logs\mesh580_prebuild_keys.txt"
"C:\Games\VItoriaBr\target-mesh580\diag\victory.exe" translate --cache --pipelines --jobs 6 "C:\Games\VItoriaBr\victory-data\game\shaders.rpf" "C:\Games\VItoriaBr\victory-data\game\game_scarlett_master.exe" "C:\Games\VItoriaBr\victory-data\game\XboxBvhBuilder.dll" "C:\Games\VItoriaBr\victory-data\game\amd_fidelityfx_upscaler_xbox.dll"
echo KEYS_EXIT=%ERRORLEVEL%
