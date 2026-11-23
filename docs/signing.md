# Code signing

The release workflow can sign Windows and macOS binaries, but signing is
**opt-in**: it runs only when the corresponding repository secrets are set.
Without them the workflow prints a warning and publishes unsigned archives, so
missing certificates never break a release.

**Current status:** the `v0.1.0` release assets were built before the signing
pipeline existed and are **unsigned**. Releases produced after the secrets below
are configured will be signed.

- Windows: Authenticode (SHA-256, RFC 3161 timestamp from DigiCert).
- macOS: Developer ID Application signature with the hardened runtime, plus
  notarization when Apple credentials are present.

The private key is never stored in the repository. Only the base64-encoded
certificate (which still contains the private key) is stored as an encrypted
GitHub Actions secret.

## Windows (Authenticode)

### 1. Get a code-signing certificate

Buy an OV or EV code-signing certificate from a CA (DigiCert, Sectigo,
GlobalSign, ...). The CA issues it as a `.pfx`/`.p12` file, or you export it
yourself from the Windows certificate store:

```powershell
# In certlm.msc: Personal > Certificates > your cert > All Tasks > Export.
# Choose "Yes, export the private key" and a password. Result: cert.pfx.
```

The certificate must have the Code Signing EKU and its private key. EV
certificates may require a hardware token; the pipeline expects a PFX, so use a
CA export that includes the private key or an OV certificate.

### 2. Base64-encode the PFX

The secret is the base64 text **without newlines**.

```powershell
# Windows PowerShell
[Convert]::ToBase64String([IO.File]::ReadAllBytes("cert.pfx"))
```

```bash
# Linux
base64 -w0 cert.pfx
# macOS (no line wrapping)
base64 -i cert.pfx | tr -d '\n'
```

Store it as `WINDOWS_CERT_PFX_BASE64` in GitHub repository secrets
(Settings > Secrets and variables > Actions), and the PFX password as
`WINDOWS_CERT_PASSWORD`.

### 3. Local signing

Requires the Windows 10/11 SDK (`signtool.exe`):

```powershell
pwsh ./scripts/sign-windows.ps1 -PfxPath ./cert.pfx -PfxPassword '<password>'
```

The script finds the newest `signtool.exe` under
`%ProgramFiles(x86)%\Windows Kits\10\bin\*\x64`, signs
`target/release/ocr-tool.exe` and `target/release/ocr-tool-gui.exe` with
`/fd SHA256`, timestamps them via `http://timestamp.digicert.com`
(`/tr ... /td SHA256`), runs `signtool verify /pa /v`, and requires
`Get-AuthenticodeSignature` to report `Valid`. Any failure aborts the script.
The password is passed only as a process argument and is never printed.

Override the inputs if needed:

```powershell
./scripts/sign-windows.ps1 `
  -Path @("target/release/ocr-tool.exe") `
  -PfxPath ./cert.pfx `
  -PfxPassword '<password>' `
  -TimestampUrl "http://timestamp.digicert.com"
```

Check a signed file:

```powershell
Get-AuthenticodeSignature .\ocr-tool.exe | Format-List Status, StatusMessage, SignerCertificate
```

## macOS (Developer ID + notarization)

### 1. Get a Developer ID Application certificate

In the Apple Developer portal create a **Developer ID Application** certificate,
then export it from Keychain Access as a password-protected `.p12`
(My Certificates > right-click > Export). The identity string looks like
`Developer ID Application: Your Name (TEAMID)`; list local identities with:

```bash
security find-identity -v -p codesigning
```

An Apple ID with an app-specific password (or an App Store Connect API key) is
required for notarization. The pipeline uses the Apple ID form:
`MACOS_NOTARY_APPLE_ID`, `MACOS_NOTARY_PASSWORD` (app-specific password) and
`MACOS_NOTARY_TEAM_ID`.

### 2. Base64-encode the P12

```bash
base64 -w0 cert.p12              # Linux
base64 -i cert.p12 | tr -d '\n'  # macOS
```

Secrets:

| Secret | Meaning |
| --- | --- |
| `MACOS_CERT_P12_BASE64` | base64 of the `.p12` with the Developer ID certificate and private key |
| `MACOS_CERT_PASSWORD` | password of the `.p12` |
| `MACOS_SIGNING_IDENTITY` | e.g. `Developer ID Application: Your Name (TEAMID)` |
| `MACOS_NOTARY_APPLE_ID` | Apple ID email (optional) |
| `MACOS_NOTARY_PASSWORD` | app-specific password (optional) |
| `MACOS_NOTARY_TEAM_ID` | Apple team ID (optional) |

If `MACOS_SIGNING_IDENTITY` is empty, the workflow skips signing with a warning.
If the three notary variables are missing, binaries are still signed but not
notarized.

### 3. Local signing

```bash
export MACOS_SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)"
export MACOS_NOTARY_APPLE_ID="you@example.com"
export MACOS_NOTARY_PASSWORD="abcd-efgh-ijkl-mnop"
export MACOS_NOTARY_TEAM_ID="TEAMID"
./scripts/sign-macos.sh
```

The script runs `codesign --force --options runtime --timestamp --sign ...` and
`codesign --verify --strict --verbose=2` on `target/release/ocr-tool` and
`target/release/ocr-tool-gui`. With all three notary variables set, it packs
both binaries into a temporary zip and runs
`xcrun notarytool submit ... --wait`.

**No `stapler` step:** these are bare Mach-O executables, not `.app`/`.dmg`
bundles, and a notarization ticket cannot be stapled to them. Gatekeeper checks
the ticket with Apple online on first launch, so the first run needs network
access. Distributing the binaries inside a signed and notarized `.app`, `.dmg`
or `.pkg` would allow stapling; this project ships bare binaries.

The hardened runtime (`--options runtime`) is required for notarization. Note
that the two executables load PDFium and start llama.cpp helpers from
`runtime/<target>/`; this pipeline signs only the two binaries. On macOS,
library validation or Gatekeeper may reject those unsigned Mach-O helpers after
a quarantined download. Before publishing a signed release, either sign the
runtime dylibs and helper executables with the same Developer ID identity (and
notarize them as well) or account for them in the entitlements; validate a
packaged build on macOS first.

Check a signed binary:

```bash
codesign -dv --verbose=4 target/release/ocr-tool
codesign --verify --strict --verbose=2 target/release/ocr-tool
spctl --assess --type execute --verbose=4 target/release/ocr-tool
```

## How the workflow behaves without secrets

In `.github/workflows/release.yml`, after the release build:

1. **Windows**: if `WINDOWS_CERT_PFX_BASE64` is empty, a
   `::warning title=Unsigned Windows build` annotation is emitted and the step
   exits successfully. Otherwise the PFX is decoded to
   `$RUNNER_TEMP/codesign.pfx`, `scripts/sign-windows.ps1` runs, and the file is
   deleted afterwards.
2. **macOS, import step**: if `MACOS_CERT_P12_BASE64` is empty, a warning is
   emitted and the step is skipped. Otherwise the P12 goes into a temporary
   keychain with a random password, and codesign is allowed to use the key.
3. **macOS, sign step**: if `MACOS_SIGNING_IDENTITY` is empty, a warning is
   emitted and the step is skipped. Otherwise `scripts/sign-macos.sh` signs and,
   when configured, notarizes.

Secrets are only referenced through `env:` and are never echoed. The temporary
PFX and keychain live in the runner's temp directory.

## Security notes

- **Never commit private keys** (`.pfx`, `.p12`, `.pem`, exported identities) or
  their passwords. Keep them out of the repository, issue archives and CI logs.
- Prefer repository or environment secrets with a narrow scope; rotate the
  certificate and secrets if a key leaks.
- Timestamping keeps signatures valid after the certificate expires, but it does
  not protect a leaked key: revoke the certificate with the CA.
- Signing proves who built the binary; it does not make the OCR output or the
  bundled runtime trusted. Model weights are not included in release archives.
