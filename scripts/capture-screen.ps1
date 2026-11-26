param([Parameter(Mandatory=$true)][string]$Output)
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
$p = Get-Process victory -EA SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if (-not $p) { 'NO_GAME_WINDOW'; exit 1 }
"PID=$($p.Id) TITLE='$($p.MainWindowTitle)'"
$b = [System.Windows.Forms.SystemInformation]::VirtualScreen
$bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($b.X, $b.Y, 0, 0, $bmp.Size)
$bmp.Save($Output, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
"PNG=$((Get-Item $Output).Length)"
