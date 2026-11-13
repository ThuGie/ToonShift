param(
    [Parameter(Mandatory)][ValidatePattern('^[A-Za-z0-9_-]+@[A-Za-z0-9_.-]+$|^[A-Za-z0-9_.-]+$')][string]$Toon,
    [Parameter(Mandatory)][ValidatePattern('^[A-Za-z0-9][A-Za-z0-9-]{0,38}/[A-Za-z0-9][A-Za-z0-9_.-]{0,98}$')][string]$Repository,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-fA-F]{40}$')][string]$Commit,
    [string]$SshConfig
)

$ErrorActionPreference = 'Stop'
$sshOptions = @()
if ($SshConfig) { $sshOptions = @('-F', $SshConfig) }
$source = "$Repository@$Commit"
& ssh @sshOptions -- $Toon "/mnt/data/toon-rust/toon-rust --sync-plugin-repo '$source'"
if ($LASTEXITCODE) { throw 'Signed catalog sync failed' }
