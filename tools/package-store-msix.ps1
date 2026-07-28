[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateSet('ShiTu', 'ShiPing')]
    [string]$Product,

    [Parameter(Mandatory)]
    [string]$ExecutablePath,

    [Parameter(Mandatory)]
    [string]$Version,

    [Parameter(Mandatory)]
    [string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'

$products = @{
    ShiTu = @{
        ExecutableName = 'ShiTu.exe'
        ManifestTemplate = Join-Path $PSScriptRoot '..\packaging\shitu\AppxManifest.xml'
        IconSource = Join-Path $PSScriptRoot '..\assets\app.png'
    }
    ShiPing = @{
        ExecutableName = 'ShiPing.exe'
        ManifestTemplate = Join-Path $PSScriptRoot '..\packaging\shiping\AppxManifest.xml'
        IconSource = Join-Path $PSScriptRoot '..\apps\shiping\assets\app.png'
    }
}

if ($Version -notmatch '^(\d+)\.(\d+)\.(\d+)$') {
    throw "Cargo version must be X.Y.Z, got '$Version'."
}

$productConfig = $products[$Product]
$msixVersion = "$Version.0"
$sourceExecutable = Resolve-Path -LiteralPath $ExecutablePath -ErrorAction Stop
$sourceExecutableName = Split-Path -Leaf $sourceExecutable.Path
if ($sourceExecutableName -cne $productConfig.ExecutableName) {
    throw "Product $Product requires executable '$($productConfig.ExecutableName)', got '$sourceExecutableName'."
}

foreach ($requiredPath in @($productConfig.ManifestTemplate, $productConfig.IconSource)) {
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
$staging = [System.IO.Path]::GetFullPath(
    (Join-Path $output "store-msix-staging-$($Product.ToLowerInvariant())")
)
$outputPrefix = $output.TrimEnd(
    [System.IO.Path]::DirectorySeparatorChar,
    [System.IO.Path]::AltDirectorySeparatorChar
) + [System.IO.Path]::DirectorySeparatorChar
if (-not $staging.StartsWith($outputPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Staging directory must remain inside the output directory: $staging"
}
$msixName = "$Product-$Version-windows-x64.msix"
$msixPath = Join-Path $output $msixName
$uploadPath = Join-Path $output "$Product-$Version-store.msixupload"
$uploadZipPath = "$uploadPath.zip"

Remove-Item -LiteralPath $staging -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath $msixPath -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath $uploadPath -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath $uploadZipPath -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path (Join-Path $staging 'Assets') -Force | Out-Null

$manifest = [System.IO.File]::ReadAllText(
    $productConfig.ManifestTemplate,
    [System.Text.UTF8Encoding]::new($false)
)
if (-not $manifest.Contains('__PACKAGE_VERSION__')) {
    throw "$($productConfig.ManifestTemplate) is missing the __PACKAGE_VERSION__ placeholder."
}
$manifest = $manifest.Replace('__PACKAGE_VERSION__', $msixVersion)
[System.IO.File]::WriteAllText((Join-Path $staging 'AppxManifest.xml'), $manifest, [System.Text.UTF8Encoding]::new($false))
Copy-Item -LiteralPath $sourceExecutable -Destination (Join-Path $staging $productConfig.ExecutableName)
Copy-Item -LiteralPath $productConfig.IconSource -Destination (Join-Path $staging 'Assets\app.png')

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
