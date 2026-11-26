@echo off
rem Offline pipeline pre-build for the RX 580 mesh->compute fallback build.
rem Isolated shader/PSO cache: victory-data\cache-mesh580-pre.
rem If the AMD compiler crashes, a later run resumes from the library saved
rem every 20 s. See RUNBOOK-mesh580.md for how to skip the offending pipeline.
set "VICTORY_CONFIG=C:\Games\VItoriaBr\victory-mesh580.toml"
set "VICTORY_CACHE_DIR=C:\Games\VItoriaBr\victory-data\cache-mesh580-pre"
set "VICTORY_LOG=info"
set "VICTORY_MESH_COMPUTE=1"
set "VICTORY_MESH_WAVE64_DAG=1"
set "VICTORY_MESH_INDIRECT=1"
set "VICTORY_SNAPSHOT_POOL_MIB=128"
set "VICTORY_D3D12_DRED=1"
set "VICTORY_PIPELINE_LOG=C:\Games\VItoriaBr\logs\mesh580_prebuild_built.log"
"C:\Games\VItoriaBr\target-mesh580\diag\victory.exe" translate --cache --pipelines --jobs 6 "C:\Games\VItoriaBr\victory-data\game\shaders.rpf" "C:\Games\VItoriaBr\victory-data\game\game_scarlett_master.exe" "C:\Games\VItoriaBr\victory-data\game\XboxBvhBuilder.dll" "C:\Games\VItoriaBr\victory-data\game\amd_fidelityfx_upscaler_xbox.dll"
echo PREBUILD_EXIT=%ERRORLEVEL%
