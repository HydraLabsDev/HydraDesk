# Releasing HydraDesk

HydraDesk follows [Semantic Versioning](https://semver.org/):

- **patch** (`0.4.x`) — bug fixes, no behaviour change
- **minor** (`0.x.0`) — backwards-compatible features
- **major** (`x.0.0`) — breaking changes

The single source of truth for the version is `version` in `cli/Cargo.toml`.

## Cut a release

1. Make sure CI is green on `main`.
2. Bump `version` in `cli/Cargo.toml`.
3. In `CHANGELOG.md`, move the `[Unreleased]` notes under a new
   `## [X.Y.Z] - YYYY-MM-DD` heading and update the compare links at the bottom.
4. Commit:
   ```bash
   git commit -am "Release vX.Y.Z"
   ```
5. Tag and push:
   ```bash
   git tag -a vX.Y.Z -m "vX.Y.Z"
   git push origin main --follow-tags
   ```
6. The **release** workflow (`.github/workflows/release.yml`) cross-builds static
   musl binaries for `x86_64`, `aarch64`, and `armv7`, then publishes a GitHub
   Release containing:
   - `hydradesk-<target>` binaries (the exact names `install.sh` downloads), and
   - a `SHA256SUMS` file.

   `install.sh` then resolves the pre-built binary automatically; no source build
   needed on the target.

## Verifying a download

```bash
sha256sum -c SHA256SUMS
```

## Quality gates

CI (`.github/workflows/ci.yml`) runs:

- `cargo build` + `cargo test` — **required**.
- `cargo fmt --check` + `cargo clippy` — **informational** for now.

Once the tree is fmt- and clippy-clean (`cargo fmt`, then `cargo clippy --fix`),
remove the `continue-on-error: true` lines from the `lint` job to make those
checks required.
