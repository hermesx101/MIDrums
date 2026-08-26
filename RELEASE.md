# MI Drums release procedure

This is the reproducible manual procedure for the MI Drums v1.0.0 Windows release. GitHub Releases
is the distribution mechanism. The repository deliberately has no installer, updater, CDN, or
automatic publishing workflow.

## 1. Prepare a clean release commit

From the repository root, review every pending change and confirm the intended branch:

```powershell
git status --short
git branch --show-current
git diff --stat
git diff --check
```

Commit the reviewed release source, documentation, licenses, community files, and retained
project-owned exercises. Do not tag or package from a dirty tree. After committing:

```powershell
git status --short
```

must produce no output. Do not use history-rewriting commands as part of release preparation.

## 2. Validate the exact commit

Run the complete gate from the clean commit that will be tagged:

```powershell
cargo fmt --check
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo doc --workspace --no-deps
cargo build --release
git diff --check
```

All commands must exit successfully. `Cargo.toml` and the two workspace packages in `Cargo.lock`
must report `1.0.0`.

## 3. Windows target and runtime requirements

The v1.0 artifact is built for Rust host target `x86_64-pc-windows-msvc` and is therefore named:

```text
MI-Drums-v1.0.0-windows-x86_64.zip
```

The application has no runtime asset/configuration dependency in the source tree. The executable
uses standard Windows libraries and imports `VCRUNTIME140.dll`. Users whose system does not already
provide it must install the official **Microsoft Visual C++ Redistributable for Visual Studio
2015-2022 (x64)** from Microsoft. Do not copy development-machine DLLs into the ZIP.

## 4. Package the portable ZIP

The release ZIP contains exactly:

```text
MI Drums/
├── MI-Drums.exe
├── README.md
├── LICENSE-MIT
└── LICENSE-APACHE
```

It does not contain source, `target/`, PDB/debug files, sample media, archives, local configuration,
or application data.

After `cargo build --release`, create a fresh staging directory and ZIP in PowerShell:

```powershell
Remove-Item -LiteralPath .\release-staging -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath .\dist -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path '.\release-staging\MI Drums' -Force | Out-Null
New-Item -ItemType Directory -Path .\dist -Force | Out-Null
Copy-Item -LiteralPath .\target\release\drums-app.exe -Destination '.\release-staging\MI Drums\MI-Drums.exe'
Copy-Item -LiteralPath .\README.md -Destination '.\release-staging\MI Drums\README.md'
Copy-Item -LiteralPath .\LICENSE-MIT -Destination '.\release-staging\MI Drums\LICENSE-MIT'
Copy-Item -LiteralPath .\LICENSE-APACHE -Destination '.\release-staging\MI Drums\LICENSE-APACHE'
Compress-Archive -LiteralPath '.\release-staging\MI Drums' -DestinationPath '.\dist\MI-Drums-v1.0.0-windows-x86_64.zip'
```

`dist/` and `release-staging/` are ignored by Git.

## 5. Generate and verify SHA-256

Only generate the checksum after the ZIP contents are final:

```powershell
$artifact = '.\dist\MI-Drums-v1.0.0-windows-x86_64.zip'
$hash = (Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash.ToLowerInvariant()
"$hash  MI-Drums-v1.0.0-windows-x86_64.zip" | Set-Content -Encoding ascii '.\dist\MI-Drums-v1.0.0-windows-x86_64.zip.sha256'
Get-FileHash -LiteralPath $artifact -Algorithm SHA256
```

Copy the same hash into `RELEASE_NOTES_v1.0.0.md` before publishing the GitHub Release. Recompute it
if the ZIP changes for any reason.

## 6. Test the packaged artifact

Test the extracted ZIP, not the executable left in `target/`:

1. Extract the ZIP to a new user-writable directory.
2. Confirm its four files exactly match the package manifest above.
3. Launch `MI-Drums.exe` and confirm the window title is **MI Drums**.
4. Repeat the verified NUX DM-1 workflow: MIDI discovery/connect, Monitor, Setup/Mapping restore,
   MIDI import, Practice, Timeline, Sheet Music, scoring, and persistence.
5. Verify Default System Device and a named output, Test Audio, metronome, backing audio, fallback,
   Preferences, Progress, Performance Analysis, and Reset All Data.
6. Run on a clean supported Windows x86_64 account/machine where practical and confirm the Visual
   C++ runtime prerequisite is handled clearly.

Record Windows version, hardware, commit ID, artifact hash, and QA result.

## 7. Windows signing status

MI Drums v1.0.0 is **unsigned**. It is not Microsoft-certified and has no established SmartScreen
reputation. Users may see a Microsoft Defender SmartScreen warning. Do not claim code signing,
certification, or installer certification in release materials. Signing is a future distribution
improvement.

## 8. Create the tag

After the clean commit and packaged artifact pass QA:

```powershell
git tag -a v1.0.0 -m "MI Drums v1.0.0"
git push origin v1.0.0
```

These commands change remote state. Run them only with release authorization; this preparation task
does not create or push the tag.

## 9. Create the GitHub Release

Create a release manually with:

- **Tag:** `v1.0.0`
- **Title:** `MI Drums v1.0.0`
- **Body:** finalized contents of `RELEASE_NOTES_v1.0.0.md`
- **Uploads:**
  - `MI-Drums-v1.0.0-windows-x86_64.zip`
  - `MI-Drums-v1.0.0-windows-x86_64.zip.sha256`

Release notes must cover what MI Drums does, major features, ZIP installation, Windows x86_64 and
Visual C++ runtime requirements, NUX DM-1 validation, known limitations, unsigned status, SHA-256,
and dual licensing. Do not attach source media, PDBs, archives, or development logs.

## 10. Verify publication

After publication:

1. Download both uploaded files from GitHub as an ordinary user.
2. Verify the downloaded ZIP against the uploaded checksum.
3. Extract and launch it on supported Windows.
4. Confirm the tag resolves to the recorded clean commit.
5. Confirm README Download guidance leads users to the repository's Releases page.
6. Confirm the release body accurately states unsigned status and known limitations.

## Final checklist

- [ ] Reviewed release commit created and working tree clean
- [ ] Version is `1.0.0` in manifest and lockfile
- [ ] Formatting, 180 tests, strict Clippy, Rustdoc, release build, and diff check pass
- [ ] Retained media provenance confirmed; questionable media absent
- [ ] License and community-health files present
- [ ] `MI-Drums-v1.0.0-windows-x86_64.zip` contains exactly four documented files
- [ ] SHA-256 generated after final ZIP creation
- [ ] Extracted ZIP tested with supported Windows and NUX DM-1 baseline
- [ ] Unsigned status and Visual C++ runtime requirement disclosed
- [ ] Clean commit tagged `v1.0.0` with authorization
- [ ] ZIP, checksum, and finalized release notes uploaded
- [ ] Published download and checksum independently verified
