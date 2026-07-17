[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$ExecutablePath,

    [Parameter(Mandatory)]
    [string]$Version,

    [Parameter(Mandatory)]
    [string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'

if ($Version -notmatch '^(\d+)\.(\d+)\.(\d+)$') {
    throw "Cargo version must be X.Y.Z, got '$Version'."
}

$msixVersion = "$Version.0"
$sourceExecutable = Resolve-Path -LiteralPath $ExecutablePath -ErrorAction Stop
$manifestTemplate = Join-Path $PSScriptRoot '..\packaging\AppxManifest.xml'
$iconSource = Join-Path $PSScriptRoot '..\assets\app.png'

foreach ($requiredPath in @($manifestTemplate, $iconSource)) {
    if (-not (Test-Path -LiteralPath $requiredPath -PathType Leaf)) {
        throw "Required packaging file is missing: $requiredPath"
    }
}

$makeAppx = (Get-Command MakeAppx.exe -ErrorAction SilentlyContinue).Source
if ([string]::IsNullOrWhiteSpace($makeAppx)) {
    $sdkRoot = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
    $makeAppx = Get-ChildItem -LiteralPath $sdkRoot -Filter MakeAppx.exe -Recurse -File -ErrorAction SilentlyContinue |
        Sort-Object FullName -Descending |
        Select-Object -First 1 -ExpandProperty FullName
}
if ([string]::IsNullOrWhiteSpace($makeAppx)) {
    throw 'MakeAppx.exe was not found. Install the Windows SDK with MSIX packaging tools.'
}

$output = [System.IO.Path]::GetFullPath($OutputDirectory)
$staging = Join-Path $output 'store-msix-staging'
$msixName = "ShiTu-$Version-windows-x64.msix"
$msixPath = Join-Path $output $msixName
$uploadPath = Join-Path $output "ShiTu-$Version-store.msixupload"
$uploadZipPath = "$uploadPath.zip"

Remove-Item -LiteralPath $staging -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath $msixPath -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath $uploadPath -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath $uploadZipPath -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path (Join-Path $staging 'Assets') -Force | Out-Null

$manifest = [System.IO.File]::ReadAllText($manifestTemplate, [System.Text.UTF8Encoding]::new($false))
if (-not $manifest.Contains('__PACKAGE_VERSION__')) {
    throw 'AppxManifest.xml is missing the __PACKAGE_VERSION__ placeholder.'
}
$manifest = $manifest.Replace('__PACKAGE_VERSION__', $msixVersion)
[System.IO.File]::WriteAllText((Join-Path $staging 'AppxManifest.xml'), $manifest, [System.Text.UTF8Encoding]::new($false))
Copy-Item -LiteralPath $sourceExecutable -Destination (Join-Path $staging 'ShiTu.exe')
Copy-Item -LiteralPath $iconSource -Destination (Join-Path $staging 'Assets\app.png')

& $makeAppx pack /o /d $staging /p $msixPath
if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $msixPath -PathType Leaf)) {
    throw 'MakeAppx failed to create the MSIX package.'
}

Compress-Archive -LiteralPath $msixPath -DestinationPath $uploadZipPath -Force
Move-Item -LiteralPath $uploadZipPath -Destination $uploadPath
if (-not (Test-Path -LiteralPath $uploadPath -PathType Leaf)) {
    throw 'Failed to create the Store upload package.'
}

Remove-Item -LiteralPath $staging -Recurse -Force

Write-Host "Created $msixPath"
Write-Host "Created $uploadPath"
