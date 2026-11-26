@echo off
rem mangrove: pre-translate every shader and pre-build the compute pipelines (victory warm)
rem with the same settings and cache folder as play.bat. Run once before the first play,
rem and again after updating mangrove. Uses every CPU core for a few minutes.
if not defined GAME set GAME=C:\path\to\your\game\game_scarlett_master.exe
if not exist "%GAME%" (
  echo Edit pc\warm.bat and set GAME to your own game executable first.
  pause
  exit /b 1
)
call "%~dp0settings.bat"
for %%I in ("%GAME%") do set GAMEDIR=%%~dpI
cd /d "%~dp0.."
target\release\victory.exe warm "%GAMEDIR%shaders.rpf" "%GAME%"
echo.
echo Warm finished with code %errorlevel%.
pause
