<!--
Copyright (c) 2026 HydraCodeLabs
Owner: HydraCodeLabs
Project: HydraDesk
SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
Last updated: 2026-06-27T01:32:17Z
-->

# Third-Party Notices

HydraDesk depends on third-party open-source packages. Their authors retain
copyright in their work, and their packages remain governed by their own
license terms.

Direct Rust dependencies declared in `cli/Cargo.toml`:

| Dependency | License family |
| --- | --- |
| `anyhow` | MIT OR Apache-2.0 |
| `clap` | MIT OR Apache-2.0 |
| `colored` | MPL-2.0 |
| `libc` | MIT OR Apache-2.0 |
| `rand` | MIT OR Apache-2.0 |
| `rpassword` | Apache-2.0 |

Builds may also include transitive dependencies resolved by Cargo. For an
exact license inventory for a release artifact, generate dependency metadata
from the resolved `Cargo.lock` used for that release.
