param(
  [Parameter(Mandatory=$true)][string]$Key,
  [string[]]$Hashes=@()
)
$list='C:\Games\VItoriaBr\victory-data\cache-mesh580-pre\pipelines\prebuilt-pipelines.txt'
if(-not (Test-Path $list)){ throw "Prebuilt list missing: $list (run prebuild-mesh580.cmd first)" }
$lines=Get-Content $list
if($lines.Count -lt 1){ throw "Prebuilt list has no tag line: $list" }
if($lines -contains $Key){ "ALREADY_PRESENT $Key" } else {
  Add-Content $list $Key -Encoding UTF8
  "ADDED $Key"
}
if($Hashes.Count -gt 0){
  $joined=($Hashes -join ',')
  Set-Content 'C:\Games\VItoriaBr\logs\mesh580_skip_hashes.txt' $joined -Encoding UTF8
  "SKIP_HASHES=$joined"
}
