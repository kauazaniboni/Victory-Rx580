@echo off
call "C:\Games\VItoriaBr\toolchain\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul || exit /b 1
set "RUSTUP_HOME=C:\Games\VItoriaBr\toolchain\rustup"
set "CARGO_HOME=C:\Games\VItoriaBr\toolchain\cargo"
set "CARGO_TARGET_DIR=C:\Games\VItoriaBr\target-mesh580"
set "PATH=C:\Games\VItoriaBr\toolchain\cargo\bin;%PATH%"
cd /d C:\Games\VItoriaBr\refs\gtxfallback
cargo build --locked --profile diag -p victory --features diagnostics
echo MESH_BUILD_EXIT=%ERRORLEVEL%
