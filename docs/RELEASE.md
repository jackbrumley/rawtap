# Release Process

RawTap follows the
[Release and Update Standard](https://github.com/jackbrumley/jackbrumley.github.io/blob/main/RELEASE_STANDARD.md),
the same process as Voquill, Rusty G6, SomeDrive and Yambuck. This is RawTap's copy of
the procedure; the standard has the reasoning and the full installer and
updater contracts.

Release identity (tag, title, asset names, download links) is consumed by
`docs/install.sh`, `docs/install.ps1` and the in-app updater, so a format
mistake breaks updates and one-line installs for every user.

## Format

| Where | Format | Example |
|---|---|---|
| `Cargo.toml` (RawTap's only version file) | `X.Y.Z` | `0.7.0` |
| Version bump commit | `chore: bump version to X.Y.Z` | |
| Git tag and release title | `vX.Y.Z` | `v0.7.0` |
| First line of the release notes | `# RawTap vX.Y.Z` | |
| Assets | `rawtap-X.Y.Z-<os>-x64[-setup].<ext>` | `rawtap-0.7.0-linux-x64.deb` |

A complete release carries five assets, all named by `scripts/release-assets.mjs`:
- `rawtap-X.Y.Z-windows-x64-setup.exe`
- `rawtap-X.Y.Z-windows-x64.msi`
- `rawtap-X.Y.Z-linux-x64.deb`
- `rawtap-X.Y.Z-linux-x64.rpm`
- `rawtap-X.Y.Z-linux-x64.AppImage`

## Branches

Development happens on `dev`; releases are tagged on `dev`. Promotion
fast-forwards `main` to the release tag, so `main` always equals the latest
release. GitHub Pages serves `brumley.info/rawtap/` (install page and
installer scripts) from `main:/docs`, so promotion also deploys installer
changes. Never commit to `main` directly.

## 1. Prepare and Tag

```bash
npm run release:prepare -- X.Y.Z
git push origin dev vX.Y.Z
```

Pushing the tag starts the **Release** workflow
(`.github/workflows/release.yml`):
1. **Prepare** checks the tag matches the `Cargo.toml` version and creates the
   pre-release with the notes skeleton.
2. **Build** runs Linux (inside the Ubuntu 22.04 container,
   `npm run release:linux`: `.deb`, `.rpm` and AppImage from one staged
   install tree) and Windows (`npm run release:windows`: NSIS `setup.exe`
   and WiX 5 MSI) in parallel and uploads the assets.
3. **Verify** fails unless the release carries exactly the five assets, each
   with a SHA-256 digest.
4. **Install test** installs the pre-release with the installer scripts from
   the tag: `.deb`, AppImage and uninstall on Linux; MSI, setup.exe and
   uninstall on Windows, checking `rawtap --version` and the PATH each time.

Follow it with:
```bash
gh run watch "$(gh run list --workflow release.yml --limit 1 --json databaseId --jq '.[0].databaseId')" --exit-status
```

## 2. Write the Release Notes

```bash
npm run --silent release:notes > release-notes.md   # commits since the last tag print to the terminal
gh release edit vX.Y.Z --notes-file release-notes.md
```

Replace every `TODO`. Structure: `# RawTap vX.Y.Z`, one summary paragraph,
themed `###` sections of `- Label: sentence` bullets separated by `---`, then
the generated Downloads block (never edited). Write for users, not developers.

## 3. Promote

```bash
npm run release:promote
```

Refuses unless all five assets have digests and the notes contain no `TODO`.
It marks the release latest and fast-forwards `main` to the tag. From then on
the in-app updater and the one-line installers serve the new version.

## 4. Verify

- An older installed version offers the update (start screen, F1 → Updates,
  `rawtap update`) and the update installs and starts the new version.
- `curl --proto '=https' --tlsv1.2 -sSf https://brumley.info/rawtap/install.sh | bash` installs the new
  version.

## When Something Fails

- **Transient failure** (network, runner): `gh run rerun <run-id> --failed`.
- **Code failure:** nothing was promoted, so nobody received it. Fix on `dev`,
  then:
  ```bash
  gh release delete vX.Y.Z --cleanup-tag --yes
  git tag -d vX.Y.Z
  git tag vX.Y.Z
  git push origin dev vX.Y.Z
  ```
- **Dry run** of every build without a release:
  `gh workflow run release.yml --ref dev`.

## Building Locally (Fallback)

- Linux (needs `podman`): `npm run release:linux`, then
  `node scripts/package-release.mjs`.
- Windows (needs NSIS and WiX 5: `choco install nsis`,
  `dotnet tool install --global wix --version 5.0.2`): `npm run release:windows`,
  then `node scripts/package-release.mjs`.
- Upload with `gh release upload vX.Y.Z release-artifacts/*`.

## Agent Checklist

- [ ] Version prepared with `npm run release:prepare`; tag is `vX.Y.Z`.
- [ ] Release workflow green: build, verify and install test.
- [ ] Notes from `npm run release:notes`; no `TODO`; Downloads block untouched.
- [ ] Assets only from the workflow or `package-release.mjs`.
- [ ] `npm run release:promote` only when the maintainer asks.
- [ ] No AI attribution in commits, tags or notes.
