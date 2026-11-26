param(
  [Parameter(Mandatory=$true)][string]$Tag,
  [string]$Launcher = 'C:\Games\VItoriaBr\run-mesh580.cmd',
  [int]$WorldTimeoutSeconds = 1800,
  [int]$WatchSeconds = 420
)
$ErrorActionPreference = 'Continue'
$game = 'C:\Games\VItoriaBr\victory-data\game\game_scarlett_master.exe'
$out = "C:\Games\VItoriaBr\logs\run_${Tag}.out"
$err = "C:\Games\VItoriaBr\logs\run_${Tag}.err"
Get-Process victory -EA SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 2
$p = Start-Process -FilePath 'cmd.exe' -ArgumentList @('/c', $Launcher, $game) -RedirectStandardOutput $out -RedirectStandardError $err -PassThru
"TRIAL=$Tag CMD_PID=$($p.Id) LAUNCHER=$Launcher"

$started = Get-Date
$deadline = $started.AddSeconds($WorldTimeoutSeconds)
$found = $false
while ((Get-Date) -lt $deadline) {
  if (Test-Path $err) {
    $hit = Select-String -Path $err -SimpleMatch -Pattern 'experimental compute and vertex mesh pipeline' -Quiet -EA SilentlyContinue
    if ($hit) { $found = $true; break }
  }
  $g = Get-Process victory -EA SilentlyContinue | Select-Object -First 1
  if (-not $g -and $p.HasExited) { break }
  Start-Sleep -Seconds 15
}
"WORLD_FOUND=$found ELAPSED=$([int]((Get-Date) - $started).TotalSeconds)"
if ($found) {
  # The first fallback pipeline can be followed by a driver fault within
  # seconds; capture immediately instead of giving the crash a head start.
  Start-Sleep -Seconds 2
  & powershell.exe -NoProfile -ExecutionPolicy Bypass -File 'C:\Games\VItoriaBr\capture-screen-any.ps1' -Output "C:\Games\VItoriaBr\logs\${Tag}_world.png"
}

$watchDeadline = (Get-Date).AddSeconds($WatchSeconds)
while ((Get-Date) -lt $watchDeadline) {
  $g = Get-Process victory -EA SilentlyContinue | Select-Object -First 1
  if (-not $g) { break }
  Start-Sleep -Seconds 20
}
$g = Get-Process victory -EA SilentlyContinue | Select-Object -First 1
if ($g) {
  & powershell.exe -NoProfile -ExecutionPolicy Bypass -File 'C:\Games\VItoriaBr\capture-screen.ps1' -Output "C:\Games\VItoriaBr\logs\${Tag}_end.png"
  "ALIVE_AT_END PID=$($g.Id) TITLE='$($g.MainWindowTitle)'"
} else {
  "PROCESS_EXITED_AT_END"
}

$c = Get-Content $err -EA SilentlyContinue | ForEach-Object { $_ -replace '\x1b\[[0-9;]*m','' }
"LINES=$($c.Count)"
foreach ($pat in @('experimental compute and vertex mesh pipeline','experimental indirect compute','mesh compute translation refused','title slot past','device removed','E_OUTOFMEMORY','unhandled exception','host pipeline refused')) {
  "COUNT $pat = $(@($c | Select-String $pat -SimpleMatch).Count)"
}
$c | Select-String 'fps, [0-9]' | Select-Object -Last 3 | ForEach-Object { "FPS $($_.Line.Substring(0, [Math]::Min(240, $_.Line.Length)))" }
$c | Select-Object -Last 12 | ForEach-Object { "TAIL $($_.Substring(0, [Math]::Min(220, $_.Length)))" }
if (Get-Process victory -EA SilentlyContinue) { Get-Process victory -EA SilentlyContinue | Stop-Process -Force }
