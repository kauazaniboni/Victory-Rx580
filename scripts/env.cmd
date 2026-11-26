@echo off
rem Loads the D: toolchain (MSVC x64 + Rust) into the current cmd session.
call "C:\Games\VItoriaBr\toolchain\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul || exit /b 1
set "RUSTUP_HOME=C:\Games\VItoriaBr\toolchain\rustup"
set "CARGO_HOME=C:\Games\VItoriaBr\toolchain\cargo"
set "CARGO_TARGET_DIR=C:\Games\VItoriaBr\target"
set "PATH=C:\Games\VItoriaBr\toolchain\cargo\bin;%PATH%"
set "VICTORY_CONFIG=C:\Games\VItoriaBr\victory.toml"
cd /d C:\Games\VItoriaBr\src
