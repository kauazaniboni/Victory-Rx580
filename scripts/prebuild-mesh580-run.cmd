@echo off
rem Detached wrapper: keeps the pre-build alive past the shell tool timeout.
call "C:\Games\VItoriaBr\prebuild-mesh580.cmd" > "C:\Games\VItoriaBr\logs\mesh580_prebuild.out" 2> "C:\Games\VItoriaBr\logs\mesh580_prebuild.err"
