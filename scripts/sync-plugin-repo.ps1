param(
    [Parameter(Mandatory)][ValidatePattern('^[A-Za-z0-9_-]+@[A-Za-z0-9_.-]+$|^[A-Za-z0-9_.-]+$')][string]$Toon,
    [Parameter(Mandatory)][ValidatePattern('^[A-Za-z0-9][A-Za-z0-9-]{0,38}/[A-Za-z0-9][A-Za-z0-9_.-]{0,98}$')][string]$Repository,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-fA-F]{40}$')][string]$Commit,
    [string]$SshConfig
)

$ErrorActionPreference = 'Stop'
$sshOptions = @()
if ($SshConfig) { $sshOptions = @('-F', $SshConfig) }
$repoName = $Repository.ToLowerInvariant().Replace('/', '--')
$root = '/mnt/data/toon-rust/catalog'
$staging = "$root/.$repoName-$([guid]::NewGuid().ToString('N')).tmp"
$destination = "$root/$repoName.json"
$temp = [System.IO.Path]::GetTempFileName()
try {
    $uri = "https://raw.githubusercontent.com/$Repository/$Commit/catalog.json"
    Invoke-WebRequest -Uri $uri -TimeoutSec 20 -OutFile $temp
    if ((Get-Item -LiteralPath $temp).Length -gt 32768) { throw 'Catalog exceeds 32 KiB' }
    $feed = Get-Content -Raw -LiteralPath $temp | ConvertFrom-Json
    if ($feed.version -ne 1 -or $feed.plugins -isnot [array] -or $feed.plugins.Count -gt 64) {
        throw 'Expected a version 1 catalog with up to 64 plugins'
    }
    $seen = @{}
    foreach ($plugin in $feed.plugins) {
        if ($plugin.id -cnotmatch '^[a-z0-9-]{1,40}$' -or $seen.ContainsKey($plugin.id) -or
            $plugin.source -cnotin @('clock', 'boiler', 'next_target', 'note', 'file') -or
            [string]::IsNullOrWhiteSpace($plugin.title) -or $plugin.description -isnot [string]) {
            throw "Invalid or duplicate widget in $Repository"
        }
        $seen[$plugin.id] = $true
    }
    & scp -O @sshOptions -- $temp "${Toon}:$staging"
    if ($LASTEXITCODE) { throw 'Catalog upload failed' }
    & ssh @sshOptions -- $Toon "chmod 600 '$staging' && mv '$staging' '$destination'"
    if ($LASTEXITCODE) { throw 'Catalog activation failed' }
    Write-Output "Synced $Repository at $Commit. Reopen Store on your Toon."
} finally {
    Remove-Item -LiteralPath $temp
}
