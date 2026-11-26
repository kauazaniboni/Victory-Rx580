# Puts the game window (victory.exe) at 1280x720, centred on the second monitor (the primary one if there is only one), without taking focus.
# (the primary one if there is no second), without activating them.
Add-Type -AssemblyName System.Windows.Forms
$s = [System.Windows.Forms.Screen]::AllScreens | Where-Object { -not $_.Primary } | Select-Object -First 1
if (-not $s) { $s = [System.Windows.Forms.Screen]::PrimaryScreen }
$a = $s.WorkingArea; $w = 1280; $hh = 720; $x = $a.Left + [int](($a.Width - $w) / 2); $y = $a.Top + [int](($a.Height - $hh) / 2)
Add-Type @"
using System; using System.Runtime.InteropServices;
public class W {
  public delegate bool P(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(P p, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int cx, int cy, uint f);
}
"@
$ids = @(Get-Process victory -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id })
$n = 0
[W]::EnumWindows({ param($h, $l) $p = [uint32]0; [void][W]::GetWindowThreadProcessId($h, [ref]$p)
  if ($ids -contains $p -and [W]::IsWindowVisible($h)) { [void][W]::SetWindowPos($h, [IntPtr]::Zero, $x, $y, $w, $hh, 0x0014); $script:n++ }  # NOZORDER|NOACTIVATE
  $true }, [IntPtr]::Zero) | Out-Null
"moved $n"
