@echo off
rem mangrove launcher: runs the game with the NVIDIA 8 GB settings and opens the easy view
rem (a plain-English status window) instead of the raw log. The game window goes to the
rem second monitor. Full raw log: %TEMP%\mangrove-game.log
rem
rem Set GAME to your own copy of the game's executable (no game files come with this repo).
if not defined GAME set GAME=C:\path\to\your\game\game_scarlett_master.exe
if not exist "%GAME%" (
  echo Edit pc\play.bat and set GAME to your own game executable first.
  pause
  exit /b 1
)
title mangrove - easy view
call "%~dp0settings.bat"
set GLOG=%TEMP%\mangrove-game.log
del "%GLOG%" 2>nul
cd /d "%~dp0.."
start "" /b cmd /c "target\release\victory.exe run "%GAME%" --execute > "%GLOG%" 2>&1"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0easy-view.ps1" -Log "%GLOG%"
