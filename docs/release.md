# Release Process

This document explains how to cut a release, how versioning works, which targets are built, and how npm publishing is handled by cargo-dist.

## 1. Release Overview

McGravity releases are fully automated through GitHub Actions and are triggered manually. The workflow:

- Uses `release-plz` to bump the Cargo version and create the git tag.
- Uses `cargo-dist` to build and package binaries for supported targets.
- Publishes a GitHub Release and npm packages from the generated artifacts.

Primary tooling:

- **cargo-dist**: build/package/release automation and npm installers.
- **release-plz**: version bumping and tagging.

Supported platforms are defined in `Cargo.toml` under `[workspace.metadata.dist]`.

## 2. Prerequisites

### GitHub secrets

- `NPM_TOKEN` (required for npm publishing)
- `CARGO_REGISTRY_TOKEN` (reserved for crates.io publishing if enabled)

### Maintainer permissions

- Write access to the repository (to push tags and release commits)
- Permission to run GitHub Actions workflows
- Permission to create GitHub Releases

### Local tooling (for troubleshooting or local dry runs)

- Rust toolchain (stable) and Cargo
- Git
- `release-plz` (optional; installed in CI for version bumps)
- `cargo-dist` (optional; for local artifact builds)
- Node.js 20+ and npm (optional; to test npm packages locally)

## 3. How to Release

The release workflow is manual and uses `workflow_dispatch` inputs.

1. Go to the repository on GitHub.
2. Open the **Actions** tab.
3. Select the **Release** workflow.
4. Click **Run workflow**.
5. Choose a version bump type: `patch`, `minor`, or `major`.
6. Click **Run workflow** to start the release.

The workflow will bump the version, create and push a tag, build artifacts, create a GitHub Release, and publish npm packages.

## 4. Version Bump Types

- **Patch (0.0.X)**: bug fixes and small changes
- **Minor (0.X.0)**: new features that are backward compatible
- **Major (X.0.0)**: breaking changes

## 5. Target Architectures

The release workflow builds binaries for the following targets:

- `x86_64-unknown-linux-gnu` (x64 Linux)
- `aarch64-unknown-linux-gnu` (ARM64 Linux)
- `x86_64-apple-darwin` (x64 macOS)
- `aarch64-apple-darwin` (ARM64 macOS / Apple Silicon)
- `x86_64-pc-windows-msvc` (x64 Windows)
- `aarch64-pc-windows-msvc` (ARM64 Windows)

## 6. npm Publishing

npm publishing is handled by `cargo-dist` with the npm installer enabled in `Cargo.toml`.

What happens:

- `cargo-dist` generates npm package artifacts ending in `-npm-package.tar.gz`.
- The `publish-npm` job downloads those artifacts and runs `npm publish --access public`.
- Package name defaults to the Cargo package name (`mcgravity`) unless a scope is configured.

Scope configuration:

- No npm scope is configured in `Cargo.toml` today.
- If a scope is added in the future via cargo-dist metadata, packages will publish under that scope.

Installation examples:

- `npm install -g mcgravity`
- `npx mcgravity`

## 7. What Happens During Release

Job sequence (from `.github/workflows/release.yml`):

```
+--------------+     +------+     +----------------------+     +----------------------+
| bump-version | --> | plan | --> | build-local-artifacts | --> | build-global-artifacts |
+--------------+     +------+     +----------------------+     +----------------------+
        |                   |                     |                         |
        |                   |                     +------------+------------+
        |                   |                                  |
        |                   +------------------------------> +------+
        |                                                     | host |
        |                                                     +------+
        |                                                        |
        |                                                        v
        |                                                   +-----------+
        |                                                   | publish-  |
        |                                                   |   npm     |
        |                                                   +-----------+
        |                                                        |
        +--------------------------------------------------------v
                                                             +----------+
                                                             | announce |
                                                             +----------+
```

Key steps explained:

1. **Version bump and tag** (`bump-version`)
   - Calculates new semver based on the chosen bump type.
   - Runs `release-plz set-version` to update `Cargo.toml`.
   - Commits the bump and tags `vX.Y.Z`.
2. **Plan** (`plan`)
   - `cargo-dist host --steps=create` computes a release plan and build matrix.
3. **Parallel builds** (`build-local-artifacts`)
   - Builds per-target binaries and installers in parallel.
4. **Global artifacts** (`build-global-artifacts`)
   - Produces global artifacts like checksums.
5. **Upload + GitHub Release** (`host`)
   - Uploads artifacts and creates the GitHub Release.
6. **npm publish** (`publish-npm`)
   - Publishes npm artifacts to the registry.
7. **Announcement** (`announce`)
   - Finalizes the workflow after publishing completes.

## 8. Troubleshooting

Common issues and fixes:

- **Missing `NPM_TOKEN`**: `publish-npm` fails. Add the secret in GitHub settings and re-run the failed job.
- **Missing write permissions**: `bump-version` cannot push tags/commits. Ensure the maintainer has write access and Actions are allowed to push.
- **Tag already exists**: A previous release tag may exist. Delete the tag (and any associated release) before re-running a full release.
- **Target build fails**: Re-run only the failed job from the Actions UI to avoid a second version bump.
- **npm version already published**: npm will reject duplicate versions. Bump to the next patch version and re-run the workflow.

Re-running guidance:

- Prefer **Re-run failed jobs** inside the same workflow run.
- Avoid starting a new release run unless you intend to bump the version again.

Manual recovery:

- If the version bump succeeded but later jobs failed, re-run the failed jobs in the same run.
- If the tag or release is incorrect, delete the tag and GitHub Release, then trigger the workflow again with the correct bump type.
