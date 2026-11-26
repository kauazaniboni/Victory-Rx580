# Turns the game's raw log into a plain-English status line every 3 seconds, plus clear alerts.
param([string]$Log)
$Host.UI.RawUI.WindowTitle = 'mangrove - what is happening (easy view)'
while (-not (Test-Path $Log)) { Start-Sleep 1 }
$fs = [IO.File]::Open($Log, 'Open', 'Read', 'ReadWrite'); $rd = New-Object IO.StreamReader($fs)
$shaders = 0; $pipes = 0; $loads = 0; $frame = '-'; $waiting = '-'; $fps = '-'; $draws = '-'; $skipped = '-'; $last = Get-Date; $start = Get-Date
function Say($color, $text) { Write-Host ("[{0:hh\:mm\:ss}] " -f ((Get-Date) - $start)) -NoNewline -ForegroundColor DarkGray; Write-Host $text -ForegroundColor $color }
Say Cyan 'Waiting for the game to start...'
while ($true) {
    $line = $rd.ReadLine()
    if ($null -eq $line) {
        if (((Get-Date) - $last).TotalSeconds -ge 3) {
            $last = Get-Date
            # the user wants the game on the second monitor: re-place it during the first 2 minutes (its window can appear late)
            if (((Get-Date) - $start).TotalSeconds -lt 120) { & "$PSScriptRoot\second-monitor.ps1" *> $null }
            Say White ("Frame {6} | Speed: {0} fps | Objects drawn: {1} | Skipped: {2} | Textures waiting for card room: {7} | Shaders ready: {3} | Graphics programs built: {4} | Files loaded: {5}" -f $fps, $draws, $skipped, $shaders, $pipes, $loads, $frame, $waiting)
            if (-not (Get-Process victory -ErrorAction SilentlyContinue)) { Say Yellow 'The game has closed.'; break }
        }
        Start-Sleep -Milliseconds 300; continue
    }
    $l = $line -replace '\x1b\[[0-9;]*m', ''
    if ($l -match '([\d.]+) fps') { $fps = $Matches[1] }
    if ($l -match '(\d+) draws/frame') { $draws = $Matches[1] }
    if ($l -match 'dropped (\d+)') { $skipped = $Matches[1] }
    if ($l -match 'frame=(\d+)') { $frame = $Matches[1] }
    if ($l -match '(\d+) waiting,') { $waiting = $Matches[1] }
    if ($l -match 'shader translated') { $shaders++ }
    if ($l -match 'first bind pipeline|pipeline made|host pipeline') { $pipes++ }
    if ($l -match 'dstorage') { $loads++ }
    if ($l -match 'device removed|0x887A0005') { Say Red 'PROBLEM: the graphics card reset (device removed). The game will stop.' }
    elseif ($l -match 'unhandled exception') { Say Red 'PROBLEM: the game hit one of its own internal checks and is stopping (crash).' }
    elseif ($l -match 'unimplemented import') { Say Red ('PROBLEM: the game called a Windows function VIctory does not have yet: ' + ($l -replace '.*called\s*', '')) }
    elseif ($l -match 'os error 1455|out of memory') { Say Red 'PROBLEM: out of memory. Close other programs or raise the Windows paging file.' }
    elseif ($l -match 'host pipeline refused') { Say Yellow 'Note: the graphics driver refused one effect program. Known and harmless: the game skips that effect.' }
    elseif ($l -cmatch 'panic| ERROR ') { Say Red ('PROBLEM: ' + ($l -replace '^\S+\s+ERROR\s+', '').Substring(0, [Math]::Min(150, $l.Length))) }
    elseif ($l -match 'library saved') { Say Green 'Saved the built graphics programs to disk (next start is faster).' }
    elseif ($l -match 'title window|window shown') { Say Green 'The game window is open.' }
}
Read-Host 'Press Enter to close'
