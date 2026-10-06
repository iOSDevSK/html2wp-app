# Authenticode-signs one file with Azure Artifact Signing (BELNEM s.r.o.).
# Tauri calls this for every binary it bundles, the NSIS uninstaller (through
# !uninstfinalize) and the installer itself: bundle.windows.signCommand, set by
# the Windows workflow. The workflow downloads SignTool and the Artifact Signing
# dlib and logs in to Azure through OIDC; the dlib authenticates as that Azure
# CLI session, so no secret is stored anywhere.
param([Parameter(Mandatory = $true)][string]$File)
$ErrorActionPreference = 'Stop'

foreach ($name in 'ARTIFACT_SIGNING_SIGNTOOL', 'ARTIFACT_SIGNING_DLIB', 'ARTIFACT_SIGNING_METADATA') {
    if (-not (Test-Path -LiteralPath ([Environment]::GetEnvironmentVariable($name)))) {
        throw "$name is not set or does not exist; run the signing setup step first"
    }
}

# The signing service and its timestamp authority are remote; retry transient failures.
for ($attempt = 1; $attempt -le 3; $attempt++) {
    & $env:ARTIFACT_SIGNING_SIGNTOOL sign /v /fd SHA256 /tr 'http://timestamp.acs.microsoft.com' /td SHA256 `
        /dlib $env:ARTIFACT_SIGNING_DLIB /dmdf $env:ARTIFACT_SIGNING_METADATA $File
    if ($LASTEXITCODE -eq 0) { exit 0 }
    Write-Warning "Signing $File failed (attempt $attempt, exit $LASTEXITCODE)"
    Start-Sleep -Seconds (5 * $attempt)
}
exit 1
