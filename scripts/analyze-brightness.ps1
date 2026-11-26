param([Parameter(Mandatory=$true)][string]$Folder,[int]$Step=4)
Add-Type -AssemblyName System.Drawing
Get-ChildItem $Folder -Filter 'frame-*.png' -File |
  Where-Object { $_.BaseName -match '^frame-\d+$' } |
  Sort-Object { [int]($_.BaseName.Substring(6)) } |
  ForEach-Object {
    $f=$_
    $b=[System.Drawing.Bitmap]::FromFile($f.FullName)
    $sum=0.0; $black=0; $n=0; $distinct=New-Object 'System.Collections.Generic.HashSet[int]'
    for($y=0; $y -lt $b.Height; $y+=$Step){
      for($x=0; $x -lt $b.Width; $x+=$Step){
        $c=$b.GetPixel($x,$y)
        $lum=[int](($c.R*3+$c.G*6+$c.B)/10)
        $sum += $lum; $n++
        if($c.R -eq 0 -and $c.G -eq 0 -and $c.B -eq 0){ $black++ }
        [void]$distinct.Add((($c.R -shr 4) -shl 8) -bor (($c.G -shr 4) -shl 4) -bor ($c.B -shr 4))
      }
    }
    $b.Dispose()
    "frame={0} AVG={1:N1} BLACK={2:N1}% DISTINCT_16={3}" -f $f.BaseName.Substring(6),($sum/$n),(100.0*$black/$n),$distinct.Count
  }
