# Changelog

All notable changes to HydraDesk are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project follows
[Semantic Versioning](https://semver.org/).

## [Unreleased]

_Nothing yet._

## [1.0.0] - 2026-06-28

First public release.

### Core
- **Headless virtual display** — forces a DP/HDMI connector on and injects a
  checksum-correct 1080p EDID so a monitor-less GNOME box renders a real desktop
  for RDP, with no dummy plug.
- **Reboot persistence** — a root display service plus a per-login autostart that
  re-stores credentials and starts GNOME Remote Desktop on every boot.
- **In-memory keyring fix** — points the secret-service `default` alias at the
  always-present `session` collection so `grdctl` can store RDP credentials on an
  auto-login device.

### Cross-distro support
- Runs on both **Debian/Ubuntu** and **Fedora/RHEL** GNOME systems. The installer
  and CLI detect the distro family and adapt automatically: `apt` vs `dnf`/`yum`
  for packages, `/etc/gdm3` vs `/etc/gdm` for auto-login, and `ufw` vs `firewalld`
  (a LAN-scoped, rate-limited rule either way) for the firewall.
- Architectures: `x86_64`, `aarch64`, `armv7`.

### Commands
- `setup`, `status`, `logs`, `doctor`, `fix`, `test`, `lock`, `unlock`,
  `uninstall`, `debug`; plus `install.sh` with `--build` and `--uninstall`.
- `hydradesk test` — a pre-flight self-check of the full connect path (session,
  framebuffer Mutter can capture, RDP enabled with credentials, remote control
  allowed, service active, port listening) so "connects then black screen"
  failures are caught before reaching for mstsc.

### Auto-lock on disconnect
- Optional: `setup` asks how many minutes after the last RDP client disconnects
  the machine should lock itself (0 = never; also settable with `--lock-timeout`).
- A per-login watcher locks the GNOME session when the grace period elapses and
  unlocks it when you reconnect; `hydradesk lock` / `hydradesk unlock` lock or
  unlock on demand. When enabled, the session is kept lockable (idle-locking
  stays off) so locking actually engages.

### Credentials & security
- Interactive prompt with confirmation, plus `--password`, `--password-stdin`,
  `--password-file`, and `--username`; weak/common passwords are refused.
- Auto-generates a strong RDP password when none is supplied; the generated
  password is shown once at the end of setup. User-supplied passwords are never
  displayed.
- LAN-scoped, rate-limited firewall rule, a public-IP warning, and the TLS
  certificate fingerprint shown in `setup` and `status`.

### Installer
- One-line `install.sh` that auto-detects the architecture, prefers a pre-built
  release, and falls back to clone-and-build (installing Rust if needed).
- Shows the version being installed and checks GitHub for a newer published
  release at the end of the run.

### Licensing
- Licensed under the **PolyForm Noncommercial License 1.0.0** — free for any
  noncommercial use; commercial use (including selling) requires a separate
  license from HydraCodeLabs. See [TERMS.md](TERMS.md) for a plain-language
  summary.

[Unreleased]: https://github.com/HydraLabsDev/HydraDesk/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/HydraLabsDev/HydraDesk/releases/tag/v1.0.0
