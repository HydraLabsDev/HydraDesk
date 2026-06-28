// ==========================================================================
// Copyright (c) 2026 HydraCodeLabs
// Owner: HydraCodeLabs
// Project: HydraDesk
// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Last updated: 2026-06-27T01:32:17Z
// ==========================================================================

use anyhow::{anyhow, bail, Context, Result};
use colored::Colorize;
use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;

use crate::detect::{
    check_port_3389, get_local_ip, is_root, run_cmd, run_cmd_inner, run_root_cmd, SessionInfo,
    SystemState,
};

// ── Installed file locations ────────────────────────────────────────────────
const SHARE_DIR: &str = "/usr/local/share/hydradesk";
const EDID_PATH: &str = "/usr/local/share/hydradesk/edid-1080p.bin";
const KEYRING_PY_PATH: &str = "/usr/local/share/hydradesk/keyring-alias.py";
const DISPLAY_INIT_PATH: &str = "/usr/local/bin/hydradesk-display-init";
const DISPLAY_SERVICE: &str = "hydradesk-display.service";
const DISPLAY_SERVICE_PATH: &str = "/etc/systemd/system/hydradesk-display.service";

// ── Public API ──────────────────────────────────────────────────────────────

pub struct RdpConfig {
    pub username: String,
    /// Set only when HydraDesk generated the password, so `setup` can show it
    /// once. Passwords the user supplied themselves are never echoed.
    pub generated_password: Option<String>,
}

/// Install everything that must survive a reboot:
///   1. the virtual-display service (headless devices only), and
///   2. the per-login autostart that re-stores credentials and starts RDP.
/// `system_user` is the Linux user whose desktop is shared (owns the files);
/// `rdp_user` / `rdp_password` are the credentials the RDP client authenticates
/// with — usually the same name, but they can differ.
pub fn install_persistence(
    state: &SystemState,
    system_user: &str,
    rdp_user: &str,
    rdp_password: &str,
    lock_timeout_secs: u64,
    verbose: bool,
) -> Result<()> {
    if !state.has_monitor {
        install_virtual_display(verbose)?;
    } else {
        println!(
            "  {} Physical monitor detected — GNOME already has a framebuffer, \
             skipping virtual display",
            "i".blue()
        );
    }

    // When auto-lock is enabled we must keep the session *lockable* (lock-enabled
    // true) even though idle-locking stays off, otherwise loginctl lock-session
    // does nothing.
    let lock_enabled = lock_timeout_secs > 0;
    install_login_autostart(system_user, rdp_user, rdp_password, lock_enabled, verbose)?;
    install_lock_watcher(system_user, lock_timeout_secs, verbose)?;
    Ok(())
}

/// Apply the full RDP configuration to the *currently running* session so the
/// user can connect immediately (no reboot needed). Requires an active session.
pub fn apply_now(
    state: &SystemState,
    rdp_user: &str,
    rdp_password: &str,
    generated: bool,
    no_firewall: bool,
    lock_enabled: bool,
    verbose: bool,
) -> Result<RdpConfig> {
    let _session = state
        .session
        .as_ref()
        .ok_or_else(|| anyhow!("No active graphical session to configure"))?;

    let grdctl = state.grdctl_path.as_deref().unwrap_or("/usr/bin/grdctl");

    // 1. Bring up the virtual display now (no-op when a real monitor exists).
    println!("  Starting virtual display...");
    start_virtual_display(state, verbose)?;

    // 2. Point the keyring 'default' alias at the in-memory session collection.
    println!("  Preparing GNOME keyring...");
    let _ = run_cmd("python3", &[KEYRING_PY_PATH], &state.session, verbose);

    // 3. Enable RDP, allow remote control, store credentials.
    println!("  Configuring GNOME Remote Desktop...");
    let _ = run_cmd(grdctl, &["rdp", "enable"], &state.session, verbose);
    let _ = run_cmd(grdctl, &["rdp", "disable-view-only"], &state.session, verbose);

    let cred = run_cmd(
        grdctl,
        &["rdp", "set-credentials", rdp_user, rdp_password],
        &state.session,
        verbose,
    )
    .context("Failed to run grdctl rdp set-credentials")?;
    if !cred.success {
        bail!(
            "grdctl set-credentials failed: {}\nRun with --debug for details.",
            cred.stderr.trim()
        );
    }

    // 4. Stop idle blanking/locking. When auto-lock is on we keep the session
    //    lockable so the disconnect watcher can lock it on demand.
    disable_screen_lock(&state.session, lock_enabled, verbose);

    // 5. (Re)start the user RDP service.
    println!("  Starting gnome-remote-desktop service...");
    let _ = run_cmd(
        "systemctl",
        &["--user", "enable", "gnome-remote-desktop.service"],
        &state.session,
        verbose,
    );
    let svc = run_cmd(
        "systemctl",
        &["--user", "restart", "gnome-remote-desktop.service"],
        &state.session,
        verbose,
    )
    .context("Failed to restart gnome-remote-desktop.service")?;
    if !svc.success {
        bail!("Failed to start gnome-remote-desktop service:\n{}", svc.stderr);
    }

    std::thread::sleep(std::time::Duration::from_millis(2000));

    // 6. Firewall — LAN-scoped + rate-limited.
    if !no_firewall {
        open_firewall_port(verbose)?;
    }

    Ok(RdpConfig {
        username: rdp_user.to_string(),
        generated_password: if generated {
            Some(rdp_password.to_string())
        } else {
            None
        },
    })
}

// ── Virtual display (headless framebuffer) ──────────────────────────────────

/// Build a valid 128-byte 1920x1080@60 EDID with a correct checksum.
fn edid_1080p() -> [u8; 128] {
    let mut e: [u8; 128] = [
        0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x31, 0xd8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x05, 0x16, 0x01, 0x03, 0x80, 0x50, 0x2d, 0x78, 0x0a, 0x0d, 0xc9, 0xa0, 0x57, 0x47, 0x98, 0x27,
        0x12, 0x48, 0x4c, 0x00, 0x00, 0x00, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
        0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x02, 0x3a, 0x80, 0x18, 0x71, 0x38, 0x2d, 0x40, 0x58, 0x2c,
        0x45, 0x00, 0xa0, 0x5a, 0x00, 0x00, 0x00, 0x1e, 0x00, 0x00, 0x00, 0xff, 0x00, 0x4c, 0x69, 0x6e,
        0x75, 0x78, 0x20, 0x23, 0x30, 0x0a, 0x20, 0x20, 0x20, 0x20, 0x00, 0x00, 0x00, 0xfd, 0x00, 0x3b,
        0x3d, 0x42, 0x44, 0x0f, 0x00, 0x0a, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x00, 0x00, 0x00, 0xfc,
        0x00, 0x4c, 0x69, 0x6e, 0x75, 0x78, 0x20, 0x46, 0x48, 0x44, 0x0a, 0x20, 0x20, 0x20, 0x00, 0x00,
    ];
    let sum: u32 = e[..127].iter().map(|&b| b as u32).sum();
    e[127] = ((256 - (sum % 256)) % 256) as u8;
    e
}

const KEYRING_PY: &str = r#"import dbus
# Point the secret-service 'default' alias at the always-present in-memory
# 'session' collection. On an autologin box no persistent 'login' keyring is
# ever unlocked, so this is what lets grdctl store RDP credentials.
try:
    bus = dbus.SessionBus()
    svc = dbus.Interface(
        bus.get_object("org.freedesktop.secrets", "/org/freedesktop/secrets"),
        "org.freedesktop.Secret.Service",
    )
    svc.OpenSession("plain", dbus.String("", variant_level=1))
    svc.SetAlias("default", dbus.ObjectPath("/org/freedesktop/secrets/collection/session"))
    print("hydradesk: keyring default->session alias set")
except Exception as e:
    print("hydradesk keyring alias error:", e)
"#;

const DISPLAY_INIT_SH: &str = r#"#!/bin/bash
# HydraDesk: bring up a virtual display so a headless GNOME session renders a
# real desktop that GNOME Remote Desktop can mirror over RDP. The GPU driver
# clears an early force when it re-probes during KMS/login bringup, so we
# re-apply only while a connector keeps reverting, and stop once it has stayed
# connected long enough for GNOME to adopt it. Capped at 120s as a backstop.
EDID=/usr/local/share/hydradesk/edid-1080p.bin

force_on() {
  for st in /sys/class/drm/card*-*/status; do
    name=$(basename "$(dirname "$st")"); name=${name#card*-}
    case "$name" in
      DP-*|HDMI-*)
        for d in /sys/kernel/debug/dri/*/"$name"/edid_override; do
          [ -e "$d" ] && [ -f "$EDID" ] && cat "$EDID" > "$d" 2>/dev/null
        done
        echo on > "$st" 2>/dev/null ;;
    esac
  done
}

# Succeeds only if at least one DP/HDMI connector exists and all are connected.
all_connected() {
  local found=1
  for st in /sys/class/drm/card*-*/status; do
    name=$(basename "$(dirname "$st")"); name=${name#card*-}
    case "$name" in
      DP-*|HDMI-*)
        found=0
        [ "$(cat "$st" 2>/dev/null)" = "connected" ] || return 1 ;;
    esac
  done
  return $found
}

end=$((SECONDS+120))
stable=0
while [ "$SECONDS" -lt "$end" ]; do
  if all_connected; then
    stable=$((stable+1))
    [ "$stable" -ge 6 ] && break   # ~18s connected with no reset -> GNOME has it
  else
    force_on
    stable=0
  fi
  sleep 3
done
"#;

const DISPLAY_SERVICE_UNIT: &str = "[Unit]\n\
Description=HydraDesk virtual display for headless RDP\n\
After=sys-kernel-debug.mount systemd-udevd.service\n\
\n\
[Service]\n\
Type=simple\n\
ExecStart=/usr/local/bin/hydradesk-display-init\n\
\n\
[Install]\n\
WantedBy=multi-user.target\n";

fn install_virtual_display(verbose: bool) -> Result<()> {
    std::fs::create_dir_all(SHARE_DIR)
        .with_context(|| format!("Failed to create {} (run with sudo)", SHARE_DIR))?;

    std::fs::write(EDID_PATH, edid_1080p())
        .with_context(|| format!("Failed to write {}", EDID_PATH))?;

    write_root_file(KEYRING_PY_PATH, KEYRING_PY, 0o644)?;
    write_root_file(DISPLAY_INIT_PATH, DISPLAY_INIT_SH, 0o755)?;
    write_root_file(DISPLAY_SERVICE_PATH, DISPLAY_SERVICE_UNIT, 0o644)?;

    let _ = run_root_cmd("systemctl", &["daemon-reload"], verbose);
    let _ = run_root_cmd("systemctl", &["enable", DISPLAY_SERVICE], verbose);

    println!(
        "  {} Virtual display service installed (1080p, auto-starts at boot)",
        "✓".green()
    );
    Ok(())
}

/// Start (or restart) the virtual display service now. No-op when a real
/// monitor is present.
pub fn start_virtual_display(state: &SystemState, verbose: bool) -> Result<()> {
    if state.has_monitor {
        return Ok(());
    }
    let _ = run_root_cmd("systemctl", &["restart", DISPLAY_SERVICE], verbose);
    std::thread::sleep(std::time::Duration::from_millis(5000));
    Ok(())
}

// ── Per-login autostart ─────────────────────────────────────────────────────

fn install_login_autostart(
    system_user: &str,
    rdp_user: &str,
    rdp_password: &str,
    lock_enabled: bool,
    _verbose: bool,
) -> Result<()> {
    let home = format!("/home/{}", system_user);
    let bin_dir = format!("{}/.local/bin", home);
    let autostart_dir = format!("{}/.config/autostart", home);

    std::fs::create_dir_all(&bin_dir)
        .with_context(|| format!("Failed to create {}", bin_dir))?;
    std::fs::create_dir_all(&autostart_dir)
        .with_context(|| format!("Failed to create {}", autostart_dir))?;

    let init_path = format!("{}/hydradesk-rdp-init", bin_dir);
    std::fs::write(&init_path, login_init_script(rdp_user, rdp_password, lock_enabled))
        .with_context(|| format!("Failed to write {}", init_path))?;
    std::fs::set_permissions(&init_path, Permissions::from_mode(0o700))?;

    let desktop_path = format!("{}/hydradesk-rdp.desktop", autostart_dir);
    let desktop = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=HydraDesk RDP\n\
         Exec={}/hydradesk-rdp-init\n\
         X-GNOME-Autostart-enabled=true\n\
         NoDisplay=true\n",
        bin_dir
    );
    std::fs::write(&desktop_path, desktop)
        .with_context(|| format!("Failed to write {}", desktop_path))?;

    let owner = format!("{}:{}", system_user, system_user);
    let _ = run_cmd_inner("chown", &["-R", &owner, &bin_dir], None, false);
    let _ = run_cmd_inner("chown", &[&owner, &autostart_dir], None, false);
    let _ = run_cmd_inner("chown", &[&owner, &desktop_path], None, false);

    println!(
        "  {} Per-login RDP setup installed (re-stores credentials each boot)",
        "✓".green()
    );
    Ok(())
}

fn login_init_script(rdp_user: &str, rdp_password: &str, lock_enabled: bool) -> String {
    let u = sh_single_quote(rdp_user);
    let p = sh_single_quote(rdp_password);
    // Keep the session lockable only when auto-lock is configured; idle-locking
    // stays off either way so the screen never locks mid-view.
    let lock_enabled_val = if lock_enabled { "true" } else { "false" };
    format!(
        "#!/bin/bash\n\
         # HydraDesk per-login RDP setup — auto-generated by 'hydradesk setup'.\n\
         # The GNOME keyring is in-memory under autologin, so credentials must be\n\
         # re-stored on every login. This file is chmod 700 — protect the device.\n\
         sleep 4\n\
         export DBUS_SESSION_BUS_ADDRESS=\"unix:path=/run/user/$(id -u)/bus\"\n\
         export XDG_RUNTIME_DIR=\"/run/user/$(id -u)\"\n\
         python3 {keyring}\n\
         grdctl rdp enable\n\
         grdctl rdp disable-view-only\n\
         grdctl rdp set-credentials {u} {p}\n\
         gsettings set org.gnome.desktop.screensaver lock-enabled {lock}\n\
         gsettings set org.gnome.desktop.screensaver idle-activation-enabled false\n\
         gsettings set org.gnome.desktop.session idle-delay 0\n\
         systemctl --user enable gnome-remote-desktop.service\n\
         systemctl --user restart gnome-remote-desktop.service\n",
        keyring = KEYRING_PY_PATH,
        u = u,
        p = p,
        lock = lock_enabled_val
    )
}

// ── Disconnect lock watcher ─────────────────────────────────────────────────

const LOCK_WATCH_REL: &str = ".local/bin/hydradesk-lock-watch";
const LOCK_AUTOSTART_REL: &str = ".config/autostart/hydradesk-lock.desktop";

/// Install the per-login watcher that locks the session some seconds after the
/// last RDP client disconnects (and unlocks it on reconnect). A timeout of 0
/// installs a watcher that disables itself, so re-running setup can toggle it.
fn install_lock_watcher(system_user: &str, timeout_secs: u64, _verbose: bool) -> Result<()> {
    let home = format!("/home/{}", system_user);
    let bin_dir = format!("{}/.local/bin", home);
    let autostart_dir = format!("{}/.config/autostart", home);
    std::fs::create_dir_all(&bin_dir)
        .with_context(|| format!("Failed to create {}", bin_dir))?;
    std::fs::create_dir_all(&autostart_dir)
        .with_context(|| format!("Failed to create {}", autostart_dir))?;

    let watch_path = format!("{}/{}", home, LOCK_WATCH_REL);
    std::fs::write(&watch_path, lock_watch_script(timeout_secs))
        .with_context(|| format!("Failed to write {}", watch_path))?;
    std::fs::set_permissions(&watch_path, Permissions::from_mode(0o755))?;

    let desktop_path = format!("{}/{}", home, LOCK_AUTOSTART_REL);
    let desktop = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=HydraDesk Lock Watch\n\
         Exec={}/hydradesk-lock-watch\n\
         X-GNOME-Autostart-enabled=true\n\
         NoDisplay=true\n",
        bin_dir
    );
    std::fs::write(&desktop_path, desktop)
        .with_context(|| format!("Failed to write {}", desktop_path))?;

    let owner = format!("{}:{}", system_user, system_user);
    let _ = run_cmd_inner("chown", &[&owner, &watch_path], None, false);
    let _ = run_cmd_inner("chown", &[&owner, &desktop_path], None, false);

    if timeout_secs == 0 {
        println!(
            "  {} Auto-lock on disconnect: off (use 'hydradesk lock' to lock manually)",
            "i".blue()
        );
    } else {
        println!(
            "  {} Auto-lock {}s after the last RDP client disconnects",
            "✓".green(),
            timeout_secs
        );
    }
    Ok(())
}

fn lock_watch_script(timeout_secs: u64) -> String {
    format!(
        "#!/bin/bash\n\
         # HydraDesk disconnect-lock watcher — auto-generated by 'hydradesk setup'.\n\
         # Locks the GNOME session TIMEOUT seconds after the last RDP client drops,\n\
         # and unlocks it when a client reconnects. TIMEOUT=0 disables locking.\n\
         TIMEOUT={timeout}\n\
         [ \"$TIMEOUT\" -le 0 ] && exit 0\n\
         \n\
         export XDG_RUNTIME_DIR=\"/run/user/$(id -u)\"\n\
         export DBUS_SESSION_BUS_ADDRESS=\"unix:path=${{XDG_RUNTIME_DIR}}/bus\"\n\
         SID=\"${{XDG_SESSION_ID:-$(loginctl list-sessions --no-legend 2>/dev/null | awk -v u=\"$(id -un)\" '$3==u{{print $1; exit}}')}}\"\n\
         \n\
         rdp_clients() {{\n\
         \x20 ss -tn 2>/dev/null | awk 'NR>1 && $1==\"ESTAB\" && $4 ~ /:3389$/' | wc -l\n\
         }}\n\
         lock_now() {{\n\
         \x20 gsettings set org.gnome.desktop.screensaver lock-enabled true 2>/dev/null\n\
         \x20 loginctl lock-session \"$SID\" 2>/dev/null \\\n\
         \x20   || gdbus call --session -d org.gnome.ScreenSaver -o /org/gnome/ScreenSaver -m org.gnome.ScreenSaver.SetActive true >/dev/null 2>&1\n\
         }}\n\
         unlock_now() {{\n\
         \x20 loginctl unlock-session \"$SID\" 2>/dev/null\n\
         \x20 gdbus call --session -d org.gnome.ScreenSaver -o /org/gnome/ScreenSaver -m org.gnome.ScreenSaver.SetActive false >/dev/null 2>&1\n\
         }}\n\
         \n\
         seen=0; locked=0; idle=0\n\
         while true; do\n\
         \x20 n=$(rdp_clients)\n\
         \x20 if [ \"$n\" -gt 0 ]; then\n\
         \x20   seen=1; idle=0\n\
         \x20   if [ \"$locked\" -eq 1 ]; then unlock_now; locked=0; fi\n\
         \x20 elif [ \"$seen\" -eq 1 ] && [ \"$locked\" -eq 0 ]; then\n\
         \x20   idle=$((idle+3))\n\
         \x20   if [ \"$idle\" -ge \"$TIMEOUT\" ]; then lock_now; locked=1; fi\n\
         \x20 fi\n\
         \x20 sleep 3\n\
         done\n",
        timeout = timeout_secs
    )
}

/// Lock the active graphical session now (used by `hydradesk lock`).
pub fn lock_session(state: &SystemState, verbose: bool) -> Result<()> {
    // Make sure the session is lockable, then send the lock.
    let _ = run_cmd(
        "gsettings",
        &["set", "org.gnome.desktop.screensaver", "lock-enabled", "true"],
        &state.session,
        verbose,
    );
    if let Some(s) = &state.session {
        let out = run_root_cmd("loginctl", &["lock-session", &s.session_id], verbose);
        if matches!(out, Ok(ref o) if o.success) {
            return Ok(());
        }
    }
    let _ = run_cmd(
        "gdbus",
        &[
            "call", "--session", "-d", "org.gnome.ScreenSaver", "-o",
            "/org/gnome/ScreenSaver", "-m", "org.gnome.ScreenSaver.SetActive", "true",
        ],
        &state.session,
        verbose,
    );
    Ok(())
}

/// Unlock the active graphical session now (used by `hydradesk unlock`).
pub fn unlock_session(state: &SystemState, verbose: bool) -> Result<()> {
    if let Some(s) = &state.session {
        let _ = run_root_cmd("loginctl", &["unlock-session", &s.session_id], verbose);
    }
    let _ = run_cmd(
        "gdbus",
        &[
            "call", "--session", "-d", "org.gnome.ScreenSaver", "-o",
            "/org/gnome/ScreenSaver", "-m", "org.gnome.ScreenSaver.SetActive", "false",
        ],
        &state.session,
        verbose,
    );
    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn write_root_file(path: &str, content: &str, mode: u32) -> Result<()> {
    std::fs::write(path, content).with_context(|| format!("Failed to write {}", path))?;
    std::fs::set_permissions(path, Permissions::from_mode(mode))
        .with_context(|| format!("Failed to chmod {}", path))?;
    Ok(())
}

/// Quote a value for safe single-quoted embedding in a generated bash script.
fn sh_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn disable_screen_lock(session: &Option<SessionInfo>, lock_enabled: bool, verbose: bool) {
    // Idle never locks the screen mid-view; idle-delay 0 means "never go idle".
    // lock-enabled stays on only when auto-lock is configured, so an explicit
    // loginctl lock-session actually engages the lock screen.
    let lock_enabled_val = if lock_enabled { "true" } else { "false" };
    let settings = [
        ("org.gnome.desktop.screensaver", "lock-enabled", lock_enabled_val),
        ("org.gnome.desktop.screensaver", "idle-activation-enabled", "false"),
        ("org.gnome.desktop.session", "idle-delay", "0"),
    ];
    for (schema, key, val) in settings {
        let _ = run_cmd("gsettings", &["set", schema, key, val], session, verbose);
    }
    if let Some(s) = session {
        let _ = run_root_cmd("loginctl", &["unlock-session", &s.session_id], verbose);
    }
}

/// Configure GDM to auto-login the given user. The config path differs by distro
/// family — /etc/gdm3 on Debian/Ubuntu, /etc/gdm on Fedora/RHEL.
pub fn configure_autologin(username: &str, _verbose: bool) -> Result<()> {
    let path = crate::detect::gdm_custom_conf();
    let config = format!(
        "[daemon]\nAutomaticLoginEnable=true\nAutomaticLogin={}\n\n[security]\n\n[xdmcp]\n\n[chooser]\n\n[debug]\n",
        username
    );
    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    std::fs::write(path, config)
        .with_context(|| format!("Failed to write {} (requires root)", path))?;
    Ok(())
}

// ── Firewall ────────────────────────────────────────────────────────────────

/// Derive the local /24 from an IPv4 address (e.g. 192.168.1.111 → 192.168.1.0/24).
fn lan_subnet_from_ip(ip: &str) -> Option<String> {
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() == 4 && parts.iter().all(|p| p.parse::<u8>().is_ok()) {
        Some(format!("{}.{}.{}.0/24", parts[0], parts[1], parts[2]))
    } else {
        None
    }
}

/// Which firewall is managing this host. HydraDesk supports the two defaults it
/// is likely to meet: ufw (Debian/Ubuntu) and firewalld (Fedora/RHEL).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Firewall {
    Ufw,
    Firewalld,
    None,
}

impl Firewall {
    /// Pick whichever firewall is actually running. firewalld is checked first
    /// because on Fedora ufw can be installed-but-inactive alongside it.
    fn detect(verbose: bool) -> Self {
        let firewalld_running = run_root_cmd("firewall-cmd", &["--state"], verbose)
            .map(|o| o.success && o.stdout.trim() == "running")
            .unwrap_or(false);
        if firewalld_running {
            return Firewall::Firewalld;
        }
        let ufw_active = run_root_cmd("ufw", &["status"], verbose)
            .map(|o| o.stdout.contains("Status: active"))
            .unwrap_or(false);
        if ufw_active {
            return Firewall::Ufw;
        }
        Firewall::None
    }
}

/// Run a firewall CLI, elevating with sudo only when we are not already root.
fn fw_run(cmd: &str, args: &[&str], verbose: bool) -> Result<crate::detect::CmdOutput> {
    if is_root() {
        run_root_cmd(cmd, args, verbose)
    } else {
        let mut a = vec![cmd];
        a.extend_from_slice(args);
        run_root_cmd("sudo", &a, verbose)
    }
}

fn ufw_run(args: &[&str], verbose: bool) -> Result<()> {
    let out = fw_run("ufw", args, verbose)?;
    if !out.success {
        bail!("ufw {} failed:\n{}", args.join(" "), out.stderr.trim());
    }
    Ok(())
}

/// Allow RDP through the active firewall — scoped to the local subnet and
/// rate-limited to blunt brute-force attempts. No-op if no firewall is active or
/// 3389 is already allowed.
pub fn open_firewall_port(verbose: bool) -> Result<()> {
    match Firewall::detect(verbose) {
        Firewall::Ufw => open_ufw_port(verbose),
        Firewall::Firewalld => open_firewalld_port(verbose),
        Firewall::None => Ok(()),
    }
}

fn open_ufw_port(verbose: bool) -> Result<()> {
    let already = run_root_cmd("ufw", &["status"], verbose)
        .map(|o| o.stdout.contains("3389"))
        .unwrap_or(false);
    if already {
        return Ok(());
    }

    let subnet = get_local_ip().and_then(|ip| lan_subnet_from_ip(&ip));
    match &subnet {
        Some(net) => {
            println!("  Allowing rate-limited RDP from {} only...", net);
            ufw_run(
                &["limit", "from", net, "to", "any", "port", "3389", "proto", "tcp"],
                verbose,
            )?;
        }
        None => {
            println!("  Allowing rate-limited RDP on 3389/tcp...");
            ufw_run(&["limit", "3389/tcp"], verbose)?;
        }
    }
    Ok(())
}

fn firewall_cmd(args: &[&str], verbose: bool) -> Result<()> {
    let out = fw_run("firewall-cmd", args, verbose)?;
    if !out.success {
        bail!(
            "firewall-cmd {} failed:\n{}",
            args.join(" "),
            out.stderr.trim()
        );
    }
    Ok(())
}

fn open_firewalld_port(verbose: bool) -> Result<()> {
    // Already allowed? --list-all shows both ports and rich rules.
    let already = run_root_cmd("firewall-cmd", &["--list-all"], verbose)
        .map(|o| o.stdout.contains("3389"))
        .unwrap_or(false);
    if already {
        return Ok(());
    }

    let subnet = get_local_ip().and_then(|ip| lan_subnet_from_ip(&ip));
    match &subnet {
        Some(net) => {
            println!("  Allowing rate-limited RDP from {} only...", net);
            // A rich rule scopes RDP to the LAN and rate-limits new matches,
            // mirroring what `ufw limit` does on Debian.
            let rule = format!(
                "rule family=\"ipv4\" source address=\"{}\" port port=\"3389\" protocol=\"tcp\" accept limit value=\"20/m\"",
                net
            );
            firewall_cmd(&["--permanent", &format!("--add-rich-rule={}", rule)], verbose)?;
        }
        None => {
            println!("  Allowing RDP on 3389/tcp...");
            firewall_cmd(&["--permanent", "--add-port=3389/tcp"], verbose)?;
        }
    }
    // Activate the permanent rule in the running config.
    firewall_cmd(&["--reload"], verbose)?;
    Ok(())
}

// ── Verification ────────────────────────────────────────────────────────────

pub struct VerifyResult {
    pub service_active: bool,
    pub port_open: bool,
}

pub fn verify(state: &SystemState, verbose: bool) -> VerifyResult {
    VerifyResult {
        service_active: check_service_active(&state.session, verbose),
        port_open: check_port_3389(verbose),
    }
}

fn check_service_active(session: &Option<SessionInfo>, verbose: bool) -> bool {
    let out = run_cmd(
        "systemctl",
        &["--user", "is-active", "gnome-remote-desktop.service"],
        session,
        verbose,
    );
    match out {
        Ok(o) => o.stdout.trim() == "active",
        Err(_) => false,
    }
}

/// Read the RDP server's TLS certificate fingerprint from grdctl, so the user
/// can verify it matches what mstsc shows (defends against MITM on connect).
pub fn tls_fingerprint(state: &SystemState, verbose: bool) -> Option<String> {
    let grdctl = state.grdctl_path.as_deref().unwrap_or("/usr/bin/grdctl");
    let out = run_cmd(grdctl, &["status"], &state.session, verbose).ok()?;
    for line in out.stdout.lines() {
        if let Some(rest) = line.trim().strip_prefix("TLS fingerprint:") {
            return Some(rest.trim().to_string());
        }
    }
    None
}

/// Print current RDP status (for `hydradesk status`).
pub fn status(verbose: bool) -> Result<()> {
    let state = SystemState::collect(verbose)?;
    let session = state.session.as_ref();

    println!("{}", "HydraDesk Status".bold().cyan());
    println!("{}", "─".repeat(50).dimmed());

    println!("  OS:            {}", state.os);
    println!("  Desktop:       {}", state.desktop);
    println!("  Display:       {}", state.display_server);

    match session {
        Some(s) => println!("  Session:       {} (uid {})", s.username, s.uid),
        None => println!("  Session:       {}", "none detected".red()),
    }

    if state.has_monitor {
        println!("  Monitor:       {}", "present".green());
    } else {
        println!("  Monitor:       {}", "headless (virtual display)".yellow());
    }

    match &state.grdctl_path {
        Some(p) => println!("  grdctl:        {}", p.green()),
        None => println!("  grdctl:        {}", "not found".red()),
    }

    let svc = check_service_active(&state.session, verbose);
    let rdp_operational = state.rdp_enabled || (svc && state.rdp_port_open);

    if state.rdp_enabled {
        println!("  RDP:           {}", "enabled".green());
    } else if rdp_operational {
        println!("  RDP:           {}", "running (grdctl status unclear)".yellow());
    } else {
        println!("  RDP:           {}", "not enabled".red());
    }

    if state.rdp_port_open {
        println!("  Port 3389:     {}", "listening".green());
    } else {
        println!("  Port 3389:     {}", "not listening".red());
    }

    if svc {
        println!("  Service:       {}", "active".green());
    } else {
        println!("  Service:       {}", "inactive".red());
    }

    if let Some(fp) = tls_fingerprint(&state, verbose) {
        println!("  TLS cert:      {}", fp.dimmed());
    }

    match &state.local_ip {
        Some(ip) => println!("  Local IP:      {}", ip.bold()),
        None => println!("  Local IP:      {}", "unknown".dimmed()),
    }

    print_connection_history(&state, verbose);

    if rdp_operational && state.rdp_port_open {
        println!();
        println!("{}", "Ready — connect from Windows:".green().bold());
        if let Some(ip) = &state.local_ip {
            println!("  mstsc → {}", ip.bold());
        }
        if let Some(s) = session {
            println!("  Username: {}", s.username.bold());
        }
    }

    Ok(())
}

// -- RDP connection history --------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum RdpEventKind {
    Connected,
    Disconnected,
}

struct RdpLogEvent {
    timestamp: String,
    epoch: Option<i64>,
    ip: String,
    kind: RdpEventKind,
}

fn print_connection_history(state: &SystemState, verbose: bool) {
    let current = current_rdp_clients(verbose);
    let events = rdp_journal_events(state, verbose);

    if current.is_empty() && events.is_empty() {
        return;
    }

    println!();
    println!("{}", "RDP connection history".bold().cyan());

    if current.is_empty() {
        println!("  Current clients: {}", "none detected".dimmed());
    } else {
        println!("  Current clients: {}", current.join(", ").green());
    }

    if let Some(last) = events.last() {
        let action = match last.kind {
            RdpEventKind::Connected => "connected",
            RdpEventKind::Disconnected => "disconnected",
        };
        println!("  Last event:      {} from {} ({})", action, last.ip.bold(), last.timestamp);
    }

    if let Some((start, end)) = last_completed_session(&events) {
        let duration = match (start.epoch, end.epoch) {
            (Some(a), Some(b)) if b >= a => format_duration(b - a),
            _ => "duration unknown".to_string(),
        };
        println!(
            "  Last session:    {} connected at {}, stayed {}",
            start.ip.bold(),
            start.timestamp,
            duration
        );
    } else if !events.is_empty() {
        println!("  Last session:    {}", "not enough log data to calculate duration".dimmed());
    }
}

fn current_rdp_clients(verbose: bool) -> Vec<String> {
    let Ok(out) = run_root_cmd("ss", &["-tn"], verbose) else {
        return Vec::new();
    };

    let mut clients = Vec::new();
    for line in out.stdout.lines().skip(1) {
        if !line.contains(":3389") {
            continue;
        }
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 5 || cols[0] != "ESTAB" {
            continue;
        }
        if let Some(ip) = extract_ipv4(cols[4]) {
            if !clients.contains(&ip) {
                clients.push(ip);
            }
        }
    }
    clients
}

fn rdp_journal_events(state: &SystemState, verbose: bool) -> Vec<RdpLogEvent> {
    let Ok(out) = run_cmd(
        "journalctl",
        &[
            "--user",
            "-u",
            "gnome-remote-desktop.service",
            "--since",
            "-30 days",
            "-o",
            "short-iso",
            "--no-pager",
        ],
        &state.session,
        verbose,
    ) else {
        return Vec::new();
    };

    out.stdout.lines().filter_map(parse_rdp_event).collect()
}

fn parse_rdp_event(line: &str) -> Option<RdpLogEvent> {
    let lower = line.to_lowercase();
    let kind = if lower.contains("disconnect")
        || lower.contains("disconnected")
        || lower.contains("connection closed")
        || lower.contains("closed connection")
        || lower.contains("client closed")
    {
        RdpEventKind::Disconnected
    } else if lower.contains("connect")
        || lower.contains("connected")
        || lower.contains("authenticated")
        || lower.contains("login")
        || lower.contains("accepted")
    {
        RdpEventKind::Connected
    } else {
        return None;
    };

    let ip = extract_ipv4(line)?;
    let timestamp = line.split_whitespace().next()?.to_string();
    let epoch = parse_short_iso_epoch(&timestamp);

    Some(RdpLogEvent {
        timestamp,
        epoch,
        ip,
        kind,
    })
}

fn last_completed_session(events: &[RdpLogEvent]) -> Option<(&RdpLogEvent, &RdpLogEvent)> {
    for end in events.iter().rev().filter(|e| e.kind == RdpEventKind::Disconnected) {
        if let Some(start) = events.iter().rev().find(|e| {
            e.kind == RdpEventKind::Connected && e.ip == end.ip && e.timestamp <= end.timestamp
        }) {
            return Some((start, end));
        }
    }
    None
}

fn extract_ipv4(s: &str) -> Option<String> {
    for part in s.split(|c: char| !(c.is_ascii_digit() || c == '.')) {
        if is_ipv4(part) {
            return Some(part.to_string());
        }
    }
    None
}

fn is_ipv4(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 4
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 3 && p.parse::<u8>().is_ok())
}

fn parse_short_iso_epoch(ts: &str) -> Option<i64> {
    let date_time = ts.get(0..19)?;
    let year = date_time.get(0..4)?.parse::<i32>().ok()?;
    let month = date_time.get(5..7)?.parse::<u32>().ok()?;
    let day = date_time.get(8..10)?.parse::<u32>().ok()?;
    let hour = date_time.get(11..13)?.parse::<i64>().ok()?;
    let minute = date_time.get(14..16)?.parse::<i64>().ok()?;
    let second = date_time.get(17..19)?.parse::<i64>().ok()?;

    let days = days_from_civil(year, month, day)?;
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

fn days_from_civil(year: i32, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    let y = year - if month <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = month as i32;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + day as i32 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era * 146097 + doe - 719468) as i64)
}

fn format_duration(seconds: i64) -> String {
    let hours = seconds / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let secs = seconds % 60;

    if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, secs)
    } else {
        format!("{}s", secs)
    }
}
// -- Password generation -----------------------------------------------------
pub fn generate_password() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    // 20 chars. No single quotes (kept shell-safe for the generated autostart
    // script). Ambiguous look-alikes (0/O, 1/l/I) are omitted.
    let chars: Vec<char> = "abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789!@#%&*+=?"
        .chars()
        .collect();
    (0..20).map(|_| chars[rng.gen_range(0..chars.len())]).collect()
}

// ── hydradesk test ────────────────────────────────────────────────────────────

/// Does Mutter currently have a monitor to capture? Queries the live display
/// config; `Some(false)` means a headless session with no framebuffer (the
/// classic "connects then black screen"). `None` means we couldn't ask.
fn mutter_has_monitor(state: &SystemState, verbose: bool) -> Option<bool> {
    let out = run_cmd(
        "gdbus",
        &[
            "call", "--session",
            "-d", "org.gnome.Mutter.DisplayConfig",
            "-o", "/org/gnome/Mutter/DisplayConfig",
            "-m", "org.gnome.Mutter.DisplayConfig.GetCurrentState",
        ],
        &state.session,
        verbose,
    )
    .ok()?;
    if !out.success {
        return None;
    }
    // An active monitor reports a current mode (`'is-current': <true>`); a
    // headless session has an empty monitor list and none.
    Some(out.stdout.contains("is-current"))
}

/// Self-check everything a client needs to connect AND see a desktop, so the
/// "black screen / can't connect" failures are caught before reaching for mstsc.
pub fn test(verbose: bool) -> Result<()> {
    use crate::detect::DesktopEnvironment;
    use crate::output::{fail, pass, section, warn};

    println!("{}", "HydraDesk Test — will a client connect and see a desktop?".bold().cyan());
    println!("{}", "─".repeat(60).dimmed());

    let state = SystemState::collect(verbose)?;
    let mut blockers = 0;

    section("Session");
    if state.desktop == DesktopEnvironment::Gnome {
        pass("GNOME desktop");
    } else {
        fail("Not GNOME", "GNOME Remote Desktop only works on GNOME.", None);
        blockers += 1;
    }
    match &state.session {
        Some(s) => pass(&format!("Active graphical session: {} (uid {})", s.username, s.uid)),
        None => {
            fail(
                "No active graphical session",
                "RDP cannot mirror a desktop that isn't running.",
                Some("Enable auto-login and reboot (sudo hydradesk setup), or log in at the console."),
            );
            blockers += 1;
        }
    }

    section("Framebuffer (the black-screen check)");
    match mutter_has_monitor(&state, verbose) {
        Some(true) => pass("GNOME is rendering to a monitor Mutter can capture"),
        Some(false) => {
            fail(
                "Mutter has no monitor — RDP would show a black screen",
                "Headless GNOME with no framebuffer to capture.",
                Some("sudo systemctl restart hydradesk-display.service"),
            );
            blockers += 1;
        }
        None => {
            if state.has_monitor {
                warn("Could not query Mutter, but a display connector is present", "", None);
            } else {
                fail(
                    "No display detected (Mutter unreachable and no DRM monitor)",
                    "RDP would show a black screen.",
                    Some("sudo systemctl restart hydradesk-display.service"),
                );
                blockers += 1;
            }
        }
    }

    section("GNOME Remote Desktop");
    let grdctl = state.grdctl_path.as_deref().unwrap_or("/usr/bin/grdctl");
    let st = run_cmd(grdctl, &["status"], &state.session, verbose)
        .map(|o| o.stdout)
        .unwrap_or_default();

    if state.rdp_enabled || st.to_lowercase().contains("status: enabled") {
        pass("RDP is enabled");
    } else {
        fail("RDP is not enabled", "", Some("sudo hydradesk setup"));
        blockers += 1;
    }
    if st.contains("(hidden)") {
        pass("Credentials are set");
    } else {
        fail("No RDP credentials stored", "grdctl has no username/password.", Some("sudo hydradesk setup"));
        blockers += 1;
    }
    if st.contains("View-only: no") {
        pass("Remote control allowed (not view-only)");
    } else if st.contains("View-only: yes") {
        warn(
            "View-only is ON — you could see the desktop but not control it",
            "",
            Some("grdctl rdp disable-view-only && systemctl --user restart gnome-remote-desktop.service"),
        );
    }

    section("Service + network");
    if check_service_active(&state.session, verbose) {
        pass("gnome-remote-desktop service is active");
    } else {
        fail("Service is not active", "", Some("systemctl --user restart gnome-remote-desktop.service"));
        blockers += 1;
    }
    if state.rdp_port_open {
        pass("Port 3389 is listening");
    } else {
        fail("Port 3389 is not listening", "", Some("hydradesk fix"));
        blockers += 1;
    }

    println!();
    println!("{}", "─".repeat(60).dimmed());
    if blockers == 0 {
        println!("{}", "✓ All checks passed — connect with mstsc and you should land on the desktop.".green().bold());
        if let Some(ip) = &state.local_ip {
            println!("  mstsc → {}   user {}", ip.bold(), state.session.as_ref().map(|s| s.username.as_str()).unwrap_or("<user>").bold());
        }
    } else {
        println!(
            "{}",
            format!("✗ {} blocker(s) found — address the items above, then re-run hydradesk test.", blockers).red().bold()
        );
    }
    Ok(())
}
