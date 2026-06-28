// ==========================================================================
// Copyright (c) 2026 HydraCodeLabs
// Owner: HydraCodeLabs
// Project: HydraDesk
// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Last updated: 2026-06-27T01:32:17Z
// ==========================================================================

use anyhow::Result;
use colored::Colorize;

use crate::detect::{DesktopEnvironment, DisplayServer, SystemState};
use crate::output::{fail, info, pass, section, warn};

pub fn run(verbose: bool) -> Result<()> {
    println!("{}", "HydraDoctor — Diagnosing your system".bold().cyan());
    println!("{}", "─".repeat(55).dimmed());

    let state = SystemState::collect(verbose)?;

    section("System");
    check_os(&state);

    section("Desktop Environment");
    check_desktop(&state);

    section("Display Server");
    check_display_server(&state);

    section("Graphical Session");
    check_session(&state);

    section("Monitor / Display Output");
    check_monitor(&state);

    section("GNOME Remote Desktop");
    check_grdctl(&state);
    check_rdp_enabled(&state, verbose);

    section("Network");
    check_port(&state);
    check_firewall(verbose);

    // Summary
    println!();
    println!("{}", "─".repeat(55).dimmed());
    summarize(&state);

    Ok(())
}

// ── Individual checks ─────────────────────────────────────────────────────────

fn check_os(state: &SystemState) {
    pass(&format!("OS detected: {}", state.os));
}

fn check_desktop(state: &SystemState) {
    match &state.desktop {
        DesktopEnvironment::Gnome => {
            pass("Desktop: GNOME");
        }
        DesktopEnvironment::Kde => {
            fail(
                "Desktop: KDE",
                "GNOME Remote Desktop is GNOME-only. KDE does not support grdctl.",
                Some("Use NoMachine or RustDesk instead for KDE."),
            );
        }
        DesktopEnvironment::Xfce => {
            fail(
                "Desktop: XFCE",
                "GNOME Remote Desktop requires GNOME.",
                Some("Use x11vnc or RustDesk instead for XFCE."),
            );
        }
        DesktopEnvironment::Lxde => {
            fail(
                "Desktop: LXDE",
                "GNOME Remote Desktop requires GNOME.",
                Some("Use x11vnc or RustDesk instead for LXDE."),
            );
        }
        DesktopEnvironment::Unknown(s) => {
            warn(
                &format!("Desktop: Unknown ({})", s),
                "Could not determine desktop environment. GNOME RDP may not work.",
                Some("Set XDG_CURRENT_DESKTOP and log in to a graphical session."),
            );
        }
    }
}

fn check_display_server(state: &SystemState) {
    match &state.display_server {
        DisplayServer::Wayland => {
            pass("Display server: Wayland");
        }
        DisplayServer::X11 => {
            warn(
                "Display server: X11",
                "GNOME Remote Desktop requires Wayland for physical session sharing.",
                Some("At the login screen, click the gear icon and select 'GNOME on Wayland'."),
            );
        }
        DisplayServer::Unknown => {
            warn(
                "Display server: Unknown",
                "Cannot determine if Wayland is running.",
                Some("Log into a Wayland GNOME session before running hydradesk setup."),
            );
        }
    }
}

fn check_session(state: &SystemState) {
    match &state.session {
        Some(s) => {
            pass(&format!(
                "Active session: user='{}' uid={} session={}",
                s.username, s.uid, s.session_id
            ));
            if let Some(d) = &s.display {
                info(&format!("  DISPLAY={}", d));
            }
            info(&format!("  DBUS={}", s.dbus_address));
        }
        None => {
            fail(
                "No active graphical session detected",
                "loginctl found no active graphical session. \
                 RDP cannot mirror a desktop that isn't running.",
                Some(
                    "Log in to the physical machine's graphical session first, \
                     then re-run this tool.",
                ),
            );
        }
    }
}

fn check_monitor(state: &SystemState) {
    if state.has_monitor {
        pass("Physical monitor detected");
    } else {
        warn(
            "No monitor detected",
            "No connected display output found. \
             Physical desktop cannot be mirrored without a display.",
            Some(
                "Connect an HDMI dummy plug, or configure a virtual display. \
                 Run: hydradesk fix --headless",
            ),
        );
    }
}

fn check_grdctl(state: &SystemState) {
    match &state.grdctl_path {
        Some(p) => pass(&format!("grdctl found: {}", p)),
        None => {
            let hint = format!(
                "Install it: {} gnome-remote-desktop",
                crate::detect::PackageManager::detect().install_hint()
            );
            fail(
                "grdctl not found",
                "gnome-remote-desktop is not installed or not in PATH.",
                Some(&hint),
            );
        }
    }
}

fn check_rdp_enabled(state: &SystemState, _verbose: bool) {
    if state.grdctl_path.is_none() {
        return;
    }
    if state.rdp_enabled {
        pass("GNOME RDP: enabled");
    } else if state.rdp_port_open {
        pass("GNOME RDP: appears running (grdctl status unclear)");
    } else {
        fail(
            "GNOME RDP: not enabled",
            "RDP is not currently enabled in GNOME Remote Desktop.",
            Some("Run: hydradesk setup"),
        );
    }
}

fn check_port(state: &SystemState) {
    if state.rdp_port_open {
        pass("Port 3389 is listening");
    } else {
        if state.rdp_enabled {
            fail(
                "Port 3389 not listening",
                "RDP is enabled but nothing is listening on 3389. \
                 The service may have failed to start.",
                Some(
                    "Check logs: journalctl --user -u gnome-remote-desktop.service -n 30",
                ),
            );
        } else {
            info("Port 3389: not listening (RDP not yet enabled)");
        }
    }
}

fn check_firewall(verbose: bool) {
    use crate::detect::run_root_cmd;

    // firewalld (Fedora/RHEL) takes precedence — on those hosts ufw is usually
    // absent or inactive.
    let firewalld_running = run_root_cmd("firewall-cmd", &["--state"], verbose)
        .map(|o| o.success && o.stdout.trim() == "running")
        .unwrap_or(false);
    if firewalld_running {
        match run_root_cmd("firewall-cmd", &["--list-all"], verbose) {
            Ok(out) if out.stdout.contains("3389") => pass("Firewall: port 3389 is allowed"),
            _ => warn(
                "Firewall: firewalld active but port 3389 not listed",
                "firewalld may be blocking RDP connections.",
                Some("Run: sudo firewall-cmd --permanent --add-port=3389/tcp && sudo firewall-cmd --reload"),
            ),
        }
        return;
    }

    match run_root_cmd("ufw", &["status"], verbose) {
        Ok(out) => {
            if out.stdout.contains("Status: inactive") || out.stdout.contains("Status: disabled") {
                info("Firewall (ufw): inactive — port 3389 is open by default");
            } else if out.stdout.contains("3389") {
                pass("Firewall: port 3389 is allowed");
            } else if out.stdout.contains("Status: active") {
                warn(
                    "Firewall: ufw active but port 3389 not listed",
                    "ufw may be blocking RDP connections.",
                    Some("Run: sudo ufw allow 3389/tcp"),
                );
            } else {
                info("Firewall: status unknown");
            }
        }
        Err(_) => {
            info("Firewall: no active ufw/firewalld (no firewall rules to check)");
        }
    }
}

// ── Summary ───────────────────────────────────────────────────────────────────

fn summarize(state: &SystemState) {
    let is_gnome = state.desktop == DesktopEnvironment::Gnome;
    let is_wayland = state.display_server == DisplayServer::Wayland;
    let has_session = state.session.is_some();
    let has_grdctl = state.grdctl_path.is_some();

    if is_gnome && is_wayland && has_session && has_grdctl {
        println!(
            "{}",
            "✓ System is ready for GNOME RDP setup.".green().bold()
        );
        println!("  Run: {}", "sudo hydradesk setup".bold().cyan());
    } else {
        println!("{}", "✗ System is NOT ready for GNOME RDP.".red().bold());
        println!("  Address the FAIL items above, then re-run hydradesk doctor.");
        if !is_gnome {
            println!("  → GNOME desktop is required");
        }
        if !is_wayland {
            println!("  → Wayland session is required (log in with GNOME on Wayland)");
        }
        if !has_session {
            println!("  → Log in to a graphical session first");
        }
        if !has_grdctl {
            println!(
                "  → Install gnome-remote-desktop: {}",
                format!(
                    "{} gnome-remote-desktop",
                    crate::detect::PackageManager::detect().install_hint()
                )
                .bold()
            );
        }
    }
}
