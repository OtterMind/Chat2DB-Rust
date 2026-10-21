# Sign one Windows installer on the shared remote Authenticode signing service.
# The code-signing certificate never leaves that service; this repository only
# carries the connection secrets. Ported from the Community packaging script of
# the same purpose so both products share one signing service.
param([Parameter(Mandatory)][string]$PackagePath)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
foreach ($name in @('WIN_SERVER_IP', 'WIN_SERVER_USER', 'WIN_SSH_PRIVATE_KEY', 'REMOTE_SIGN_PATH', 'REMOTE_SIGN_SCRIPT', 'HOST_KEY')) {
    if (-not [Environment]::GetEnvironmentVariable($name)) { throw "Missing signing configuration: $name" }
}
$winScp = Get-Command WinSCP.com -ErrorAction SilentlyContinue
if ($winScp) {
    $executable = $winScp.Source
} else {
    $executable = (Get-Item "${env:ProgramFiles(x86)}/WinSCP/WinSCP.com" -ErrorAction Stop).FullName
}
$source = (Resolve-Path -LiteralPath $PackagePath).Path
$extension = [IO.Path]::GetExtension($source)
function Quote-WinScp([string]$value) { '"' + $value.Replace('"', '""') + '"' }
function Quote-Remote([string]$value) { "'" + $value.Replace("'", "'\''") + "'" }
function Invoke-WinScp([string[]]$commands) {
    $scriptFile = [IO.Path]::GetTempFileName()
    try {
        # WinSCP has its own command parser; native argv escaping can corrupt embedded quotes.
        [IO.File]::WriteAllLines($scriptFile, $commands, [Text.UTF8Encoding]::new($true))
        & $executable /ini=nul "/script=$scriptFile" | Write-Host
        return $LASTEXITCODE
    } finally {
        Remove-Item -LiteralPath $scriptFile -Force -ErrorAction SilentlyContinue
    }
}
$user = [Uri]::EscapeDataString($env:WIN_SERVER_USER)
$password = [Uri]::EscapeDataString($env:WIN_SSH_PRIVATE_KEY)
$session = "scp://${user}:${password}@$env:WIN_SERVER_IP"
$signer = Quote-Remote $env:REMOTE_SIGN_SCRIPT
$open = ('open ' + (Quote-WinScp $session) + ' -hostkey=' + (Quote-WinScp $env:HOST_KEY) + ' -rawsettings SendBuf=0')
$maxAttempts = 3

# Upload and sign. A failed upload leaves no reusable state, so retry the pair
# with a fresh remote name.
$remote = ''
$uploaded = $false
for ($attempt = 1; $attempt -le $maxAttempts -and -not $uploaded; $attempt++) {
    $operation = [guid]::NewGuid().ToString('N')
    $remote = $env:REMOTE_SIGN_PATH.TrimEnd('/') + '/chat2db-rust-' + $operation + $extension
    $remoteFile = Quote-Remote $remote
    $exit = Invoke-WinScp @(
        'option batch abort'
        'option confirm off'
        $open
        ('put ' + (Quote-WinScp $source) + ' ' + (Quote-WinScp $remote))
        ("call $signer --alg SHA-1 $remoteFile")
        ("call $signer --alg SHA-256 $remoteFile")
        'exit'
    )
    if ($exit -eq 0) {
        $uploaded = $true
    } else {
        Write-Warning "Signing upload attempt $attempt failed with exit code $exit"
        Start-Sleep -Seconds 15
    }
}
if (-not $uploaded) { throw 'Remote package signing failed.' }

# Download the signed file separately so a transfer interruption retries only
# the download instead of repeating the upload and both signature calls.
$signed = Join-Path ([IO.Path]::GetTempPath()) ('chat2db-rust-signed-' + [guid]::NewGuid().ToString('N') + $extension)
$downloaded = $false
for ($attempt = 1; $attempt -le $maxAttempts -and -not $downloaded; $attempt++) {
    $exit = Invoke-WinScp @(
        'option batch abort'
        'option confirm off'
        $open
        ('get ' + (Quote-WinScp $remote) + ' ' + (Quote-WinScp $signed))
        ('rm ' + (Quote-WinScp $remote))
        'exit'
    )
    if ($exit -eq 0) {
        $downloaded = $true
    } else {
        Write-Warning "Signed download attempt $attempt failed with exit code $exit"
        Start-Sleep -Seconds 15
    }
}
if (-not $downloaded) {
    Remove-Item -LiteralPath $signed -Force -ErrorAction SilentlyContinue
    throw 'Remote package signing failed.'
}

try {
    $signature = Get-AuthenticodeSignature -LiteralPath $signed
    if ($signature.Status -ne [System.Management.Automation.SignatureStatus]::Valid) {
        throw "Signed package verification failed: $($signature.Status)"
    }
    Move-Item -LiteralPath $signed -Destination $source -Force
} finally {
    Remove-Item -LiteralPath $signed -Force -ErrorAction SilentlyContinue
}
