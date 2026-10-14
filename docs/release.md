# Release & Publishing Guide

LanLink can be built and published to GitHub Releases with a single command.

## Requirements

- `git` and the [GitHub CLI (`gh`)](https://cli.github.com/)
- Authenticated: `gh auth login`
- Platform-specific build tools (the script skips platforms it cannot build):
  - `.deb` — `dpkg-deb` (Debian/Ubuntu)
  - `.rpm` — `rpmbuild`, or Docker/Podman with a Fedora image
  - `.pkg` — `pkgbuild` (macOS only)
  - `.msi` — `cargo-xwin` + WiX on Linux/macOS, or WiX natively on Windows

## One-command publish

```bash
# Publish the workspace version from Cargo.toml
make dist-publish

# Or specify a version explicitly
make dist-publish VERSION=0.2.0

# Include GUI front-ends in the Linux .deb build
LL_GUI=all make dist-publish VERSION=0.2.0
```

This will:

1. Read the version from `Cargo.toml` (or `VERSION=`).
2. Build every package the current host supports.
3. Create and push a git tag `v<version>`.
4. Create a GitHub release.
5. Upload the built artifacts.

## Common options

```bash
# Dry-run: build everything but do not create/publish the release
make dist-publish VERSION=0.2.0 PUBLISH_OPTS="--dry-run"

# Use artifacts already in dist/*/out/
make dist-publish VERSION=0.2.0 PUBLISH_OPTS="--skip-build"

# Skip tag creation/push (useful if the tag already exists)
make dist-publish VERSION=0.2.0 PUBLISH_OPTS="--skip-tag"

# Build and publish only specific package types
make dist-publish VERSION=0.2.0 PUBLISH_OPTS="--platforms deb,msi"

# Custom release notes
make dist-publish VERSION=0.2.0 PUBLISH_OPTS='--notes "Bug fixes and performance improvements"'
make dist-publish VERSION=0.2.0 PUBLISH_OPTS="--notes-file CHANGELOG.md"

# Mark as prerelease
make dist-publish VERSION=0.2.0-beta.1 PUBLISH_OPTS="--prerelease"
```

## Cross-platform releases

The publish script only builds what the current machine can build. For a full
release with every platform, you have two options:

### Option A: run on each platform

Run `make dist-publish VERSION=0.2.0` on:

- a Linux host (produces `.deb`, `.rpm` if docker/rpmbuild, `.msi` if cargo-xwin + WiX)
- a macOS host (produces `.pkg`)
- a Windows host (produces `.msi`)

For the CI-only tag push path, use `make dist-publish-ci VERSION=0.2.0` instead.

### Option B: CI matrix + one upload job

1. Build each artifact in a CI matrix job on the appropriate runner.
2. Collect all artifacts into a single job.
3. Run `bash dist/publish-release.sh --skip-build VERSION=0.2.0` to upload them.

```yaml
# Example GitHub Actions upload step
- uses: actions/download-artifact@v4
  with:
    path: dist-artifacts
    pattern: '*'
- run: |
    cp dist-artifacts/deb/*.deb dist/deb/out/
    cp dist-artifacts/rpm/*.rpm dist/rpm/out/
    cp dist-artifacts/pkg/*.pkg dist/macos/out/
    cp dist-artifacts/msi/*.msi dist/windows/out/
    bash dist/publish-release.sh --skip-build VERSION=${GITHUB_REF_NAME#v}
  env:
    GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

## Manual per-platform builds

```bash
make build-deb       # Debian/Ubuntu package
make build-rpm       # Fedora/RHEL package
make build-pkg       # macOS installer
make build-msi       # Windows installer
make dist            # build all packages the host can produce
make dist-clean      # remove build trees
```

## Troubleshooting

- `gh: command not found` — install the GitHub CLI and run `gh auth login`.
- `gh release create` fails — ensure the tag is unique and you have write access.
- Missing `.rpm` on Debian/Ubuntu — install `docker.io` or `podman` and the
  script will build the RPM in a Fedora container.
- Missing `.msi` on Linux — install `cargo-xwin` (`cargo install cargo-xwin`) and
  WiX (`dotnet tool install --global wix`).
