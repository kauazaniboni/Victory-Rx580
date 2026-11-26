param([Parameter(Mandatory=$true)][string]$Folder,[int]$Step=4)
Add-Type -AssemblyName System.Drawing
$files=Get-ChildItem $Folder -Filter 'frame-*.png' -File | Where-Object { $_.BaseName -match '^frame-\d+$' }
$frames=@{}
foreach($f in $files){
  $n=[int]($f.BaseName.Substring(6))
  $b=[System.Drawing.Bitmap]::FromFile($f.FullName)
  $w=$b.Width;$h=$b.Height
  $gray=New-Object 'byte[]' ([int][Math]::Ceiling($w/$Step)*[int][Math]::Ceiling($h/$Step))
  $i=0
  for($y=0;$y -lt $h;$y+=$Step){ for($x=0;$x -lt $w;$x+=$Step){ $c=$b.GetPixel($x,$y); $gray[$i]=[byte](($c.R*3+$c.G*6+$c.B)/10); $i++ } }
  $b.Dispose()
  $frames[$n]=$gray
}
$keys=@($frames.Keys | Sort-Object)
$d1=@{};$d2=@{}
foreach($n in $keys){
  if($frames.ContainsKey($n-1)){
    $a=$frames[$n];$b=$frames[$n-1];$s=0
    for($i=0;$i -lt $a.Length;$i++){ $s += [Math]::Abs([int]$a[$i]-[int]$b[$i]) }
    $d1[$n]=$s/$a.Length
  }
  if($frames.ContainsKey($n-2)){
    $a=$frames[$n];$b=$frames[$n-2];$s=0
    for($i=0;$i -lt $a.Length;$i++){ $s += [Math]::Abs([int]$a[$i]-[int]$b[$i]) }
    $d2[$n]=$s/$a.Length
  }
}
$exact=0;$rollback=0;$back=0;$events=@()
foreach($n in $keys){
  if($d1.ContainsKey($n) -and $d1[$n] -eq 0){ $exact++ }
  if($d1.ContainsKey($n) -and $d2.ContainsKey($n)){
    if($d2[$n] -lt $d1[$n]){ $back++ }
    if($d2[$n] -eq 0 -and $d1[$n] -ge 1.0){
      $rollback++
      if($events.Count -lt 80){ $events += ("frame={0} d1={1:N2} d2={2:N2}" -f $n,$d1[$n],$d2[$n]) }
    }
  }
}
$sum=0;$cnt=0
foreach($n in $keys){ if($d1.ContainsKey($n)){ $sum += $d1[$n]; $cnt++ } }
"FRAMES=$($keys.Count) FIRST=$($keys[0]) LAST=$($keys[-1]) AVG_D1=$(if($cnt){[math]::Round($sum/$cnt,3)}else{0}) EXACT=$exact BACK=$back ROLLBACK=$rollback"
$events | ForEach-Object { $_ }
