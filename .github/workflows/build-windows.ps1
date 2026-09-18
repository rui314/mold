$ErrorActionPreference = 'Stop'
Set-Location "$PSScriptRoot/../.."

cargo build --release --locked --package mold-cli
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$metadata = cargo metadata --locked --no-deps --format-version 1 | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$version = ($metadata.packages | Where-Object name -eq 'mold-cli').version

$stage = 'mold-install'
New-Item -ItemType Directory -Force "$stage/bin", "$stage/share/man/man1", "$stage/share/doc/mold", dist | Out-Null
Copy-Item target/release/mold.exe "$stage/bin/"
Copy-Item docs/mold.1 "$stage/share/man/man1/"
Copy-Item LICENSE "$stage/share/doc/mold/"
Compress-Archive -Path "$stage/*" -DestinationPath "dist/mold-$version-x86_64-windows.zip" -Force
