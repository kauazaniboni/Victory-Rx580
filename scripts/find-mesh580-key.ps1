param([Parameter(Mandatory=$true)][string]$Hash)
$keys='C:\Games\VItoriaBr\logs\mesh580_prebuild_keys.txt'
if(-not (Test-Path $keys)){ throw "Keys file missing: $keys" }
Get-Content $keys | Where-Object { $_ -like "*$Hash*" } | ForEach-Object {
  $parts=$_ -split "`t"
  "KEY=$($parts[0])"
  "LINE=$_"
}
