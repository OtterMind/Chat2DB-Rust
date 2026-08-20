$ErrorActionPreference = "Stop"

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$targetRoot = Join-Path $repositoryRoot "target"
$outputDirectory = Join-Path $targetRoot "windows-runtime"
$stagingDirectory = Join-Path $targetRoot ".windows-runtime.staging-$PID"
$moduleFile = Join-Path $repositoryRoot "packaging/macos/jlink-modules.txt"

function Remove-SafeDirectory([string]$Path) {
    if (Test-Path -LiteralPath $Path) {
        $item = Get-Item -LiteralPath $Path -Force
        if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw "Refusing to remove unsafe directory: $Path"
        }
        Remove-Item -LiteralPath $Path -Recurse -Force
    }
}

if (-not $env:JAVA_HOME) {
    throw "JAVA_HOME must point to the JDK 17 installation"
}
$javaHome = (Resolve-Path $env:JAVA_HOME).Path
$jlink = Join-Path $javaHome "bin/jlink.exe"
$java = Join-Path $javaHome "bin/java.exe"
$jmods = Join-Path $javaHome "jmods"
if (-not (Test-Path -LiteralPath $jlink) -or -not (Test-Path -LiteralPath $java) -or -not (Test-Path -LiteralPath $jmods)) {
    throw "JAVA_HOME must contain java.exe, jlink.exe, and jmods"
}
if (-not (Test-Path -LiteralPath $moduleFile)) {
    throw "Missing jlink module list: $moduleFile"
}

$modules = @(
    Get-Content -LiteralPath $moduleFile |
        ForEach-Object { $_.Trim() } |
        Where-Object { $_ -and -not $_.StartsWith("#") } |
        Sort-Object -Unique
) -join ","
if (-not $modules) {
    throw "jlink module list is empty"
}

New-Item -ItemType Directory -Force -Path $targetRoot | Out-Null
Remove-SafeDirectory $stagingDirectory
New-Item -ItemType Directory -Force -Path $stagingDirectory | Out-Null

& $jlink `
    --module-path $jmods `
    --add-modules $modules `
    --bind-services `
    --strip-debug `
    --no-header-files `
    --no-man-pages `
    --compress=2 `
    --output $stagingDirectory
if ($LASTEXITCODE -ne 0) {
    throw "jlink failed with exit code $LASTEXITCODE"
}

& (Join-Path $stagingDirectory "bin/java.exe") -version
if ($LASTEXITCODE -ne 0) {
    throw "Generated Windows Java runtime failed its version check"
}
$moduleNames = (& (Join-Path $stagingDirectory "bin/java.exe") --list-modules | ForEach-Object { $_.Split("@")[0] })
foreach ($module in ($modules -split ",")) {
    if ($moduleNames -notcontains $module) {
        throw "Generated runtime is missing required module $module"
    }
}

Remove-SafeDirectory $outputDirectory
Move-Item -LiteralPath $stagingDirectory -Destination $outputDirectory
Write-Host "Built Windows Java 17 runtime at $outputDirectory"
