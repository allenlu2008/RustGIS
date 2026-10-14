# 虾神格式转换：构建 plugin.wasm 并打包成 .rgplugin
#
#   .\build.ps1            构建 wasm 并打包
#   .\build.ps1 -SkipBuild 复用上次构建，仅复制并打包
#
# 打包不用 Compress-Archive：PowerShell 5.1 的它用反斜杠写 zip 条目名，
# 而 RustGIS 的包读取器按正斜杠匹配 plugin.json，这里用 .NET ZipArchive 显式控制条目名。
$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath $PSScriptRoot

if (-not $SkipBuild) {
    cargo build --release --target wasm32-unknown-unknown
    if ($LASTEXITCODE -ne 0) { throw 'cargo build 失败' }
}

$wasm = Join-Path $PSScriptRoot 'target\wasm32-unknown-unknown\release\xiashen_format_converter.wasm'
if (-not (Test-Path $wasm)) { throw "找不到 $wasm" }
Copy-Item $wasm (Join-Path $PSScriptRoot 'plugin.wasm') -Force

$version = (Get-Content (Join-Path $PSScriptRoot 'plugin.json') -Raw -Encoding UTF8 | ConvertFrom-Json).version
if ([string]::IsNullOrWhiteSpace($version)) { throw 'plugin.json 里读不到 version' }
$out = Join-Path $PSScriptRoot ("xiashen-format-converter-$version.rgplugin")
if (Test-Path $out) { Remove-Item $out }

Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [System.IO.Compression.ZipFile]::Open($out, [System.IO.Compression.ZipArchiveMode]::Create)
try {
    foreach ($name in 'plugin.json', 'plugin.wasm') {
        $entry = $zip.CreateEntry($name)
        $src = [System.IO.File]::OpenRead((Join-Path $PSScriptRoot $name))
        try {
            $dst = $entry.Open()
            try { $src.CopyTo($dst) } finally { $dst.Dispose() }
        } finally { $src.Dispose() }
    }
} finally {
    $zip.Dispose()
}
Write-Host "已打包: $out ($([math]::Round((Get-Item $out).Length / 1KB, 1)) KB)"
