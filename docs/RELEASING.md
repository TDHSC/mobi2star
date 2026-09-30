# Releasing

Pushing a `v*` tag runs `.github/workflows/release.yml`, which builds, attests and publishes a GitHub release.

## Steps

1. In `CHANGELOG.md`, rename `## Unreleased` to `## X.Y.Z — short title`. The release notes are taken from this section, and the workflow fails if it is missing.
2. Set `version = "X.Y.Z"` under `[workspace.package]` in `Cargo.toml`, then run `cargo update --workspace` so `Cargo.lock` picks up the new version.
3. Commit as `Release X.Y.Z`, push, and wait for CI to pass.
4. Tag and push:

   ```sh
   git tag -a vX.Y.Z -m "mobi2star X.Y.Z"
   git push origin vX.Y.Z
   ```

## What the workflow does

The `check` job:

1. Fails unless the tag is exactly `v` followed by the crate version.
2. Fails unless `CHANGELOG.md` has notes for that version (`tools/release-notes.sh`).
3. Runs the full quality gate (`tools/qa.sh`).

The `build` job then compiles a release binary on a native runner for each target:

| Target | Runner |
|---|---|
| `aarch64-apple-darwin` | `macos-latest` |
| `x86_64-apple-darwin` | `macos-15-intel` |
| `x86_64-unknown-linux-musl` | `ubuntu-latest` |
| `aarch64-unknown-linux-musl` | `ubuntu-24.04-arm` |

Each binary is smoke-tested before packaging: `--version`, then `convert` and `verify` on a fixture. Linux binaries must also be statically linked. Each target is packaged as `mobi2star-vX.Y.Z-TARGET.tar.gz`, which contains the binary, `README.md`, `LICENSE` and `CHANGELOG.md`.

Finally, the `publish` job:

1. Writes `SHA256SUMS`.
2. Records a build-provenance attestation for every archive.
3. Creates the release. A version with a pre-release suffix, such as `-alpha.1`, is marked as a pre-release.

## Browser page

The same workflow publishes the browser page to <https://tdhsc.github.io/mobi2star/>, after the release:

1. The `site` job runs `.github/workflows/web.yml` once `check` passes. It builds the site (`tools/build-web.sh`) and runs the WebAssembly smoke test against the CLI. On a tag it also adds the currently published release's `v/<version>/` files (`tools/keep-published-release.sh`), so pages opened before the update keep working, and uploads the Pages artifact.
2. The `pages` job deploys that artifact to the `github-pages` environment, but only after `publish` has succeeded. A tag whose release fails therefore publishes no page.

To redeploy a release, re-run the `site` job of its release run from the Actions page. The `pages` job depends on it and re-runs with it.

The repository needs a one-time setup before the first deployment:
- enable Pages with GitHub Actions as the source (`gh api -X POST repos/TDHSC/mobi2star/pages -f build_type=workflow`);
- allow `v*` tags to deploy to the `github-pages` environment, whose default rule admits only the default branch.

## Dry run

Start the workflow manually to build and smoke-test every archive and the browser page without publishing anything:

```sh
gh workflow run release.yml
```

The archives are attached to the workflow run as artifacts.

## Checking a downloaded archive

```sh
shasum -a 256 -c SHA256SUMS --ignore-missing
gh attestation verify mobi2star-vX.Y.Z-TARGET.tar.gz --repo TDHSC/mobi2star
```

The macOS binaries are not signed or notarized. Remove the quarantine flag after extracting:

```sh
xattr -d com.apple.quarantine mobi2star
```
