# Release verification

Every release produced by `.github/workflows/release.yml` publishes, next to the
platform archives:

- **`SHA256SUMS`** — SHA-256 checksums of every archive;
- **GitHub build provenance attestations** — one Sigstore bundle per archive,
  created by `actions/attest-build-provenance@v2` and stored in the repository.

**Current status:** `v0.1.0` was built before the attestation pipeline; its
`SHA256SUMS` asset was added manually, but it has **no** provenance
attestations. Releases from the next tag include both. Archives are still
**unsigned** unless the optional Authenticode/Developer ID secrets are
configured; see [docs/signing.md](signing.md).

## 1. Download the archive and `SHA256SUMS`

On the [Releases](https://github.com/onive/ocr-tool/releases) page open the
release and download:

- `ocr-tool-<version>-<target>.zip` (Windows) or `.tar.gz` (Linux/macOS);
- `SHA256SUMS` from the same release.

## 2. Check the checksum

### Windows (PowerShell)

`Get-FileHash` prints uppercase hex; `SHA256SUMS` uses lowercase, and PowerShell
string comparison is case-insensitive anyway. Compare the values:

```powershell
$archive = '.\ocr-tool-<version>-windows-x64.zip'
$expected = (Select-String -Path .\SHA256SUMS -Pattern ([regex]::Escape((Split-Path -Leaf $archive)))).Line.Split()[0]
$actual = (Get-FileHash -Algorithm SHA256 $archive).Hash
if ($actual -eq $expected) { 'OK' } else { 'MISMATCH' }
```

Or read both values and compare them by eye:

```powershell
Get-FileHash -Algorithm SHA256 .\ocr-tool-<version>-windows-x64.zip
Get-Content .\SHA256SUMS
```

### Linux

From the folder containing the archive and `SHA256SUMS`:

```bash
sha256sum -c SHA256SUMS
```

This expects every archive listed in `SHA256SUMS` to be in the current folder.
To check a single downloaded archive instead:

```bash
grep 'windows-x64.zip$' SHA256SUMS | sha256sum -c -
```

### macOS

`sha256sum` is not included in stock macOS; use `shasum`:

```bash
shasum -a 256 -c SHA256SUMS
```

Single archive:

```bash
grep 'macos-arm64.tar.gz$' SHA256SUMS | shasum -a 256 -c -
```

Each line ends with `OK` when the file matches. Any mismatch means the download
is corrupted or modified: delete it and download again.

## 3. Verify the provenance attestation

The checksum alone cannot tell you who produced the archive — `SHA256SUMS`
could be replaced together with the archive. The attestation binds the archive
digest to the GitHub Actions workflow run that built it.

This step does not apply to `v0.1.0`: it has no attestations, so only the
checksum in section 2 can be checked. The command below works from the next
tag on.

Install the [GitHub CLI](https://cli.github.com/) (the `attestation` command
appeared in `gh` 2.49) and run, in the folder with the downloaded archive:

```bash
gh attestation verify ocr-tool-<version>-windows-x64.zip --repo onive/ocr-tool
```

On success it reports the archive digest and `Verification succeeded`. `gh`
checks that:

- the attestation signature is valid and the bundle is present in GitHub's
  attestation store for the repository;
- the attestation was issued by this repository's release workflow
  (`.github/workflows/release.yml`, OIDC identity of the GitHub Actions run);
- the SHA-256 digest in the attestation matches the archive you downloaded.

Use the actual file name, for example
`ocr-tool-<version>-macos-arm64.tar.gz` or `ocr-tool-<version>-linux-x64.tar.gz`.
`--repo` is required because the verification looks up attestations of that
repository.

`gh attestation verify` returns a non-zero exit code on any failure, so it can
be used in scripts.

## What these checks do and do not prove

| Check | Proves | Does not prove |
| --- | --- | --- |
| `SHA256SUMS` | the archive was not corrupted or altered after the checksum file was generated | who built it; the checksum file itself is unauthenticated |
| Attestation | the archive is exactly the artifact built by this repository's release workflow for the recorded commit | that the code is safe, bug-free or signed |
| Authenticode / Developer ID (optional) | the publisher identity in the OS trust UI | that the build is reproducible or the OCR output is correct |

Attestations and checksums are **not code signatures** in the operating-system
sense. They do **not** remove Windows SmartScreen or macOS Gatekeeper warnings:
the executables are still unsigned unless the signing secrets described in
[docs/signing.md](signing.md) are configured. Provenance is also unavailable for
locally built binaries — only archives downloaded from Releases have it.

Verify a Windows signature (when present):

```powershell
Get-AuthenticodeSignature .\ocr-tool.exe | Format-List Status, SignerCertificate
```

Verify a macOS signature (when present):

```bash
codesign --verify --strict --verbose=2 ./ocr-tool
```
