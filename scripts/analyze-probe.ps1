param([Parameter(Mandatory=$true)][string]$Path,[int]$MinLevel=40)
$lines=Get-Content $Path
$frames=@{}
foreach($line in $lines){
  $p=$line -split '\s+'
  if($p.Count -ge 3){
    $n=[int]$p[0]; $hash=$p[1]; $level=[int]$p[2]
    $frames[$n]=@{H=$hash;L=$level}
  }
}
$keys=@($frames.Keys | Sort-Object)
$unique=@($frames.Values | ForEach-Object { $_.H } | Sort-Object -Unique).Count
$exact=0;$rollback=0;$rollbackLit=0;$events=@();$lit=@()
foreach($n in $keys){
  $f=$frames[$n]
  if($frames.ContainsKey($n-1) -and $f.H -eq $frames[$n-1].H){ $exact++ }
  if($frames.ContainsKey($n-1) -and $frames.ContainsKey($n-2) -and $f.H -eq $frames[$n-2].H -and $f.H -ne $frames[$n-1].H){
    $rollback++
    if($f.L -ge $MinLevel -or $frames[$n-1].L -ge $MinLevel){
      $rollbackLit++
      if($events.Count -lt 60){ $events += ("frame={0} level {1}<-{2} hash={3}" -f $n,$f.L,$frames[$n-1].L,$f.H.Substring(0,8)) }
    }
  }
  if($f.L -ge $MinLevel -and $lit.Count -lt 10){ $lit += "frame=$n level=$($f.L)" }
}
"FRAMES=$($keys.Count) FIRST=$($keys[0]) LAST=$($keys[-1]) UNIQUE=$unique EXACT_DUP=$exact ROLLBACK_A_B_A=$rollback ROLLBACK_LIT=$rollbackLit"
"FIRST_LIT=$($lit -join ', ')"
$events | ForEach-Object { $_ }
