<!--
Copyright (c) 2026 HydraCodeLabs
Owner: HydraCodeLabs
Project: HydraDesk
SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
Last updated: 2026-06-28T00:00:00Z
-->

# HydraDesk v1.0.0 — first public release

**RDP into your headless GNOME/Linux box from Windows `mstsc` — and land on the *real* desktop.** One command, reboot-proof, no cloud, no extra client, no monitor required.

```bash
curl -fsSL https://raw.githubusercontent.com/HydraLabsDev/HydraDesk/main/install.sh | bash
```

## Why it exists
Getting Windows to remote into Linux *and see the genuine desktop* is a rabbit hole — XRDP spawns a separate session, VNC needs a real framebuffer, and the cloud tools relay your pixels through someone else's servers. GNOME Remote Desktop is the right engine, but on a headless auto-login box it fails silently on three buried issues. HydraDesk knows about all three and fixes them automatically.

## Highlights
- **Headless virtual display** — injects a checksum-correct 1080p EDID and forces a DP/HDMI connector on, so a monitor-less box renders a real desktop. No dummy plug.
- **Reboot-persistent** — a root display service + per-login autostart re-arm everything (display, credentials, RDP) on every boot.
- **In-memory keyring fix** — re-aliases the secret-service collection so `grdctl` can store RDP credentials on an auto-login device.
- **Cross-distro** — runs on **Debian/Ubuntu** *and* **Fedora/RHEL** GNOME systems; auto-detects `apt`/`dnf`, `/etc/gdm3` / `/etc/gdm`, and `ufw`/`firewalld`.
- **`hydradesk test`** — pre-flight self-check that walks the whole connect path (session, framebuffer, RDP, credentials, service, port) so "connects then black screen" is caught *before* you open mstsc.
- **Secure by default** — LAN-scoped, rate-limited firewall rule; NLA + TLS; weak passwords refused; auto-generated strong password shown once; public-IP warning; TLS fingerprint in `setup`/`status`.

## Install options
| Method | Command |
| --- | --- |
| One-line | `curl -fsSL …/install.sh \| bash` |
| Build on device | `git clone … && cd HydraDesk && bash install.sh --build` |
| Manual (cargo) | `cargo build --release --manifest-path cli/Cargo.toml` |

Architectures: `x86_64`, `aarch64`, `armv7` (static musl binaries attached to the release).

## Verify your download
```bash
sha256sum -c SHA256SUMS
```

## Requirements
GNOME on **Wayland**, `gnome-remote-desktop` ≥ 42, systemd, and `apt` or `dnf`. Not for KDE/XFCE/LXDE or X11-only sessions — `hydradesk doctor` tells you exactly why if your system isn't supported.

## Commands
`setup` · `status` · `test` · `doctor` · `fix` · `logs` · `uninstall` · `debug`

## License
PolyForm Noncommercial 1.0.0 — free for noncommercial use; commercial use needs a separate license. See **[TERMS.md](../TERMS.md)** (plain-language) and **[LICENSE](../LICENSE)**.
