@echo off
rem Settings shared by play.bat and warm.bat (the shader cache is keyed by them: keep them the same).
set VICTORY_LOG=info,victory_gpu::descriptor::host=error
rem NVIDIA: no repeated frames (fork #269), no black mosaic blocks (#310), card buffers sized for 8 GB
set VICTORY_TEXTURE_GUARD=0
set VICTORY_SHADER_OPT=0
rem card buffers: the fork's default (1,024 MB) draws the first room's walls; 512 left textures waiting
rem VICTORY_TEXTURE_ROOM_PERCENT=94 gives textures the room the bigger buffers take (16-26k fewer waiting)
set VICTORY_TEXTURE_ROOM_PERCENT=94
rem copy-backs settled later: 2.3 fps against 1.5, copy-back 218 ms against 498 (RTX 3070 overnight runs)
set VICTORY_LATE_WAIT=2
rem shorter per-frame wait for pipelines still being built (run 4: 2.5 fps against 2.3)
set VICTORY_PIPELINE_WAIT_MS=24
rem the title stops on an int3 when a fixed pool is full (~6-8 min into the opening cinematic, VIctory #698):
rem make those allocations fail with null instead (see README, "Getting past the 8-minute crash")
set VICTORY_POOL_NO_ASSERT=1
if not defined VICTORY_CACHE_DIR set VICTORY_CACHE_DIR=%LOCALAPPDATA%\mangrove\cache
if not defined VICTORY_DATA_DIR set VICTORY_DATA_DIR=%LOCALAPPDATA%\mangrove\data
