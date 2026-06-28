// ==========================================================================
// Copyright (c) 2026 HydraCodeLabs
// Owner: HydraCodeLabs
// Project: HydraDesk
// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Last updated: 2026-06-27T01:32:17Z
// ==========================================================================

use anyhow::{bail, Result};
use colored::Colorize;
use std::io::{IsTerminal, Read, Write};
use std::path::PathBuf;

use crate::detect::{is_root, DesktopEnvironment, DisplayServer, SystemState};
use crate::output::{banner, section};
use crate::rdp::{
    apply_now, configure_autologin, install_persistence, open_firewall_port, tls_fingerprint,
    verify, RdpConfig,
};

// ── hydradesk setup ───────────────────────────────────────────────────────────

pub fn run(
    password_arg: Option<String>,
    password_stdin: bool,
    password_file: Option<PathBuf>,
    username_arg: Option<String>,
    no_firewall: bool,
    lock_timeout: Option<u64>,
    verbose: bool,
) -> Result<()> {
    banner();
    println!("{}", "HydraDesk Setup".bold().cyan());
    println!("{}", "─".repeat(55).dimmed());
    println!();

    // ── Step 0: Must be root ─────────────────────────────────────────────────
    if !is_root() {
        bail!("hydradesk setup must be run as root.  Try:  sudo hydradesk setup");
    }

    // ── Step 1: Collect system state ─────────────────────────────────────────
    println!("{}", "Detecting system...".dimmed());
    let state = SystemState::collect(verbose)?;
    print_system_summary(&state);

    // Loud warning if this box looks internet-facing.
    if let Some(ip) = &state.local_ip {
        if ip_is_public(ip) {
            println!();
            println!("{}", "  WARNING: this device appears to have a PUBLIC IP address.".red().bold());
            println!("{}", "  Never expose RDP (port 3389) directly to the internet.".red());
            println!("{}", "  Put it behind a VPN / SSH tunnel and keep RDP on your LAN.".yellow());
        }
    }
    println!();

    // ── Step 2: Must be GNOME ────────────────────────────────────────────────
    if state.desktop != DesktopEnvironment::Gnome {
        println!(
            "{} GNOME not detected (found: {}).",
            "Setup cannot continue:".red().bold(),
            state.desktop
        );
        println!("GNOME Remote Desktop only works with GNOME.");
        println!("Alternatives: NoMachine (nomachine.com) or RustDesk (rustdesk.com)");
        return Ok(());
    }

    // ── Auto-lock preference ─────────────────────────────────────────────────
    // Collected BEFORE the password prompt: rpassword puts the terminal into a
    // no-echo/raw mode and (on some systems) doesn't fully restore it, which
    // would make this numeric prompt invisible and hang on Enter. Asking first,
    // while the terminal is pristine, avoids that entirely.
    section("Auto-lock on disconnect");
    let lock_timeout_minutes = resolve_lock_timeout(lock_timeout);
    let lock_timeout_secs = lock_timeout_minutes.saturating_mul(60);
    if lock_timeout_minutes == 0 {
        println!("  {} Disabled — the screen stays unlocked after you disconnect.", "i".blue());
    } else {
        println!(
            "  {} Screen will lock {} minute(s) after the last RDP client disconnects.",
            "✓".green(),
            lock_timeout_minutes
        );
    }

    // ── Step 3: Install dependencies ─────────────────────────────────────────
    section("Installing dependencies");
    install_deps(verbose);

    // ── Step 4: RDP credentials (prompt unless provided on the command line) ──
    section("RDP credentials");
    let password_arg = read_password_source(password_arg, password_stdin, password_file)?;
    let (system_user, rdp_user, rdp_password, generated) =
        resolve_credentials(password_arg, username_arg, &state)?;
    // rpassword can leave the tty without echo / without CR->NL translation;
    // restore a sane line discipline so any later prompt behaves normally.
    restore_terminal();
    println!("  {} RDP username: {}", "✓".green(), rdp_user.bold());

    // ── Step 5: Configure GDM auto-login (for the system user) ───────────────
    section("Configuring auto-login");
    match configure_autologin(&system_user, verbose) {
        Ok(()) => println!("  {} GDM auto-login configured for {}", "✓".green(), system_user.bold()),
        Err(e) => println!("  {} Could not configure auto-login: {}", "!".yellow(), e),
    }

    // ── Step 6: Install reboot-persistent pieces ─────────────────────────────
    section("Installing HydraDesk services");
    install_persistence(&state, &system_user, &rdp_user, &rdp_password, lock_timeout_secs, verbose)?;

    // ── Step 7: Apply now if a session is live, else defer to next boot ───────
    let has_session = state.display_server == DisplayServer::Wayland && state.session.is_some();

    if has_session {
        section("Activating now");
        let config = apply_now(
            &state,
            &rdp_user,
            &rdp_password,
            generated,
            no_firewall,
            lock_timeout_secs > 0,
            verbose,
        )?;

        section("Verifying");
        let v = verify(&state, verbose);
        if v.service_active {
            println!("  {} gnome-remote-desktop service is active", "✓".green());
        } else {
            println!("  {} gnome-remote-desktop service is NOT active", "✗".red());
        }
        if v.port_open {
            println!("  {} Port 3389 is listening", "✓".green());
        } else {
            println!("  {} Port 3389 is NOT listening", "✗".red());
        }

        println!();
        if v.service_active || v.port_open {
            print_success(&state, &config);
        } else {
            print_failure_hint();
        }
    } else {
        section("No active session — will activate on next boot");
        println!("  No live Wayland session was found. HydraDesk has installed");
        println!("  everything needed; it will configure RDP automatically on the");
        println!("  next login (auto-login is now enabled).");

        if !no_firewall {
            let _ = open_firewall_port(verbose);
        }

        print_deferred_instructions(&rdp_user, &rdp_password, generated, &state);
        offer_reboot();
    }

    Ok(())
}

// ── Credential resolution ─────────────────────────────────────────────────────

/// Returns (system_user, rdp_user, rdp_password).
/// `system_user` is the Linux account whose desktop is shared (auto-login + file
/// ownership); the RDP username/password are what the client authenticates with.
fn resolve_credentials(
    password_arg: Option<String>,
    username_arg: Option<String>,
    state: &SystemState,
) -> Result<(String, String, String, bool)> {
    let system_user = state
        .session
        .as_ref()
        .map(|s| s.username.clone())
        .or_else(|| std::env::var("SUDO_USER").ok())
        .or_else(|| std::env::var("USER").ok())
        .unwrap_or_else(|| "user".to_string());

    let interactive = std::io::stdin().is_terminal();

    // Username
    let rdp_user = match username_arg {
        Some(u) if !u.trim().is_empty() => u.trim().to_string(),
        _ => {
            if interactive {
                let entered = prompt_line(&format!("  RDP username [{}]: ", system_user));
                if entered.is_empty() {
                    system_user.clone()
                } else {
                    entered
                }
            } else {
                system_user.clone()
            }
        }
    };

    // Password. `generated` is true only when HydraDesk created it, so setup can
    // show it once. Passwords the user supplies are never echoed.
    let (rdp_password, generated) = match password_arg {
        Some(p) => {
            if let Some(reason) = is_weak_password(&p) {
                bail!("Refusing weak RDP password ({}). Choose a stronger one.", reason);
            }
            (p, false)
        }
        None => {
            if interactive {
                match prompt_password_interactive()? {
                    Some(p) => (p, false),
                    None => (crate::rdp::generate_password(), true),
                }
            } else {
                // No password supplied and no terminal to prompt: generate a
                // strong one (it is printed at the end of setup).
                (crate::rdp::generate_password(), true)
            }
        }
    };

    Ok((system_user, rdp_user, rdp_password, generated))
}

/// Minutes of "no RDP client" before the screen auto-locks. Prompts when not
/// supplied on the command line and a terminal is available; otherwise 0 (off).
fn resolve_lock_timeout(arg: Option<u64>) -> u64 {
    if let Some(m) = arg {
        return m;
    }
    if std::io::stdin().is_terminal() {
        let entered = prompt_line(
            "  Lock the screen how many minutes after the last RDP client disconnects? [0 = never]: ",
        );
        entered.trim().parse::<u64>().unwrap_or(0)
    } else {
        0
    }
}

/// Best-effort restore of a sane terminal line discipline (echo on, CR->NL
/// translation on) after a no-echo password prompt. Only acts on a real tty.
fn restore_terminal() {
    if std::io::stdin().is_terminal() {
        // `stty` inherits our stdin (the controlling tty) and resets it.
        let _ = std::process::Command::new("stty").arg("sane").status();
    }
}

fn read_password_source(
    password_arg: Option<String>,
    password_stdin: bool,
    password_file: Option<PathBuf>,
) -> Result<Option<String>> {
    if password_stdin {
        let mut password = String::new();
        std::io::stdin().read_to_string(&mut password)?;
        return Ok(Some(trim_trailing_newlines(password)));
    }

    if let Some(path) = password_file {
        let password = std::fs::read_to_string(&path)?;
        return Ok(Some(trim_trailing_newlines(password)));
    }

    Ok(password_arg)
}

fn trim_trailing_newlines(mut value: String) -> String {
    while value.ends_with('\n') || value.ends_with('\r') {
        value.pop();
    }
    value
}

/// Prompt for a password. Returns `Ok(None)` if the user leaves it blank, which
/// means "generate a strong one for me".
fn prompt_password_interactive() -> Result<Option<String>> {
    loop {
        let p1 = rpassword::prompt_password(
            "  RDP password (leave blank to auto-generate a strong one): ",
        )?;

        if p1.trim().is_empty() {
            return Ok(None);
        }
        if let Some(reason) = is_weak_password(&p1) {
            println!("  {} Too weak: {}. Try again.", "!".yellow(), reason);
            continue;
        }
        let p2 = rpassword::prompt_password("  Confirm password: ").unwrap_or_default();
        if p1 != p2 {
            println!("  {} Passwords did not match. Try again.", "!".yellow());
            continue;
        }
        return Ok(Some(p1));
    }
}

/// Reject obviously weak passwords (too short or containing a common word).
fn is_weak_password(pw: &str) -> Option<String> {
    if pw.chars().count() < 12 {
        return Some("must be at least 12 characters".to_string());
    }
    let lower = pw.to_lowercase();
    let common = [
        "testpassword", "password", "passw0rd", "letmein", "changeme", "admin",
        "123456", "qwerty", "hydradesk", "raspberry", "jetson", "ubuntu",
    ];
    if common.iter().any(|c| lower.contains(c)) {
        return Some("contains a common/guessable word".to_string());
    }
    None
}

/// True only for a clearly internet-routable IPv4 (used to warn before exposing RDP).
fn ip_is_public(ip: &str) -> bool {
    let oct: Vec<u8> = ip.split('.').filter_map(|x| x.parse().ok()).collect();
    if oct.len() != 4 {
        return false; // not IPv4 → don't raise a false alarm
    }
    let (a, b) = (oct[0], oct[1]);
    let private = a == 10
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
        || a == 127
        || (a == 169 && b == 254)
        || (a == 100 && (64..=127).contains(&b)); // CGNAT
    !private
}

fn prompt_line(prompt: &str) -> String {
    print!("{}", prompt);
    let _ = std::io::stdout().flush();
    let mut s = String::new();
    let _ = std::io::stdin().read_line(&mut s);
    s.trim().to_string()
}

// ── Dependency installation ───────────────────────────────────────────────────

fn install_deps(verbose: bool) {
    use crate::detect::{binary_exists, run_cmd_inner, PackageManager};

    let needs_grd = !binary_exists("grdctl");

    let needs_pydbus = std::process::Command::new("python3")
        .args(["-c", "import dbus"])
        .output()
        .map(|o| !o.status.success())
        .unwrap_or(true);

    if !needs_grd && !needs_pydbus {
        println!("  {} All dependencies present", "✓".green());
        return;
    }

    // gnome-remote-desktop and the python3 dbus bindings share the same package
    // names on both Debian and Fedora; only the package manager differs.
    match PackageManager::detect() {
        PackageManager::Apt => {
            let _ = run_cmd_inner("apt-get", &["update", "-qq"], None, false);
            if needs_grd {
                println!("  Installing gnome-remote-desktop...");
                let _ = run_cmd_inner(
                    "apt-get",
                    &["install", "-y", "-q", "gnome-remote-desktop"],
                    None,
                    verbose,
                );
            }
            if needs_pydbus {
                println!("  Installing python3-dbus...");
                let _ = run_cmd_inner(
                    "apt-get",
                    &["install", "-y", "-q", "python3-dbus"],
                    None,
                    verbose,
                );
            }
        }
        PackageManager::Dnf => {
            if needs_grd {
                println!("  Installing gnome-remote-desktop...");
                let _ = run_cmd_inner(
                    "dnf",
                    &["install", "-y", "gnome-remote-desktop"],
                    None,
                    verbose,
                );
            }
            if needs_pydbus {
                println!("  Installing python3-dbus...");
                let _ = run_cmd_inner("dnf", &["install", "-y", "python3-dbus"], None, verbose);
            }
        }
        PackageManager::Unknown => {
            println!(
                "  {} No supported package manager (apt/dnf) found.",
                "!".yellow()
            );
            println!("    Install manually: gnome-remote-desktop, python3-dbus");
        }
    }
}

// ── Output helpers ────────────────────────────────────────────────────────────

fn print_system_summary(state: &SystemState) {
    println!("  OS:       {}", state.os);
    println!("  Desktop:  {}", state.desktop);
    println!("  Session:  {}", state.display_server);
    if let Some(s) = &state.session {
        println!("  User:     {} (uid {})", s.username, s.uid);
    }
    println!(
        "  Monitor:  {}",
        if state.has_monitor {
            "present".to_string()
        } else {
            "headless -> virtual display".to_string()
        }
    );
    if let Some(ip) = &state.local_ip {
        println!("  IP:       {}", ip.bold());
    }
}

fn print_success(state: &SystemState, config: &RdpConfig) {
    let ip = state.local_ip.as_deref().unwrap_or("<your-linux-ip>");
    let session_kind = if state.has_monitor {
        "Physical Desktop (Wayland)"
    } else {
        "Physical Desktop @ 1080p (headless virtual display)"
    };

    println!("{}", "+----------------------------------------------+".cyan());
    println!("{}", "|          HydraDesk Setup Complete            |".cyan().bold());
    println!("{}", "+----------------------------------------------+".cyan());
    println!();
    println!("  {:<16} {}", "RDP Mode:".dimmed(), "GNOME Remote Desktop");
    println!("  {:<16} {}", "Session:".dimmed(), session_kind.green());
    println!("  {:<16} {}", "Survives reboot:".dimmed(), "yes".green().bold());
    println!("  {:<16} {}", "Status:".dimmed(), "READY".green().bold());
    println!();
    println!("{}", "Connect from Windows:".bold());
    println!();
    println!("  Open Run (Win+R):   {}", "mstsc".bold().cyan());
    println!("  Computer:           {}", ip.bold().cyan());
    println!("  Username:           {}", config.username.bold());
    match &config.generated_password {
        Some(pw) => {
            println!("  Password:           {}", pw.bold().yellow());
            println!(
                "  {}",
                "(auto-generated — save it now; it will not be shown again)".yellow()
            );
        }
        None => println!("  Password:           {}", "the password you set".dimmed()),
    }
    println!();
    if let Some(fp) = tls_fingerprint(state, false) {
        println!("  {} {}", "TLS fingerprint:".dimmed(), fp.dimmed());
        println!("  {}", "(verify this matches the certificate mstsc shows on first connect)".dimmed());
        println!();
    }
    println!("{}", "You will see the SAME desktop as a directly-attached monitor.".green());
    println!("{}", "Keep this device on your LAN — do not port-forward 3389.".yellow());
}

fn print_deferred_instructions(username: &str, password: &str, generated: bool, state: &SystemState) {
    let ip = state.local_ip.as_deref().unwrap_or("<your-linux-ip>");

    println!("{}", "+----------------------------------------------+".cyan());
    println!("{}", "|        HydraDesk — Reboot to Activate        |".cyan().bold());
    println!("{}", "+----------------------------------------------+".cyan());
    println!();
    println!("  After rebooting, connect from Windows:");
    println!();
    println!("  Open Run (Win+R):   {}", "mstsc".bold().cyan());
    println!("  Computer:           {}", ip.bold().cyan());
    println!("  Username:           {}", username.bold());
    if generated {
        println!("  Password:           {}", password.bold().yellow());
        println!();
        println!("{}", "  Auto-generated — save it now; it will not be shown again.".yellow());
    } else {
        println!("  Password:           {}", "the password you set".dimmed());
        println!();
        println!("{}", "  Save your password now; HydraDesk will not print it again.".dimmed());
    }
}

fn print_failure_hint() {
    println!("{}", "Setup ran but verification failed.".yellow().bold());
    println!();
    println!("Diagnostics:");
    println!("  journalctl --user -u gnome-remote-desktop.service -n 50 --no-pager");
    println!("  grdctl rdp status");
    println!("  ss -tulnp | grep 3389");
    println!();
    println!("Then re-run:  {}", "hydradesk doctor".bold());
}

fn offer_reboot() {
    println!();
    println!("{}", "Reboot now to activate? (y/N)".bold());
    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_ok() && input.trim().eq_ignore_ascii_case("y") {
        println!("Rebooting...");
        let _ = std::process::Command::new("reboot").status();
    }
}

// ── hydradesk lock / unlock ───────────────────────────────────────────────────

pub fn lock(verbose: bool) -> Result<()> {
    let state = SystemState::collect(verbose)?;
    if state.session.is_none() {
        bail!("No active graphical session to lock.");
    }
    crate::rdp::lock_session(&state, verbose)?;
    println!("  {} Screen locked.", "✓".green());
    Ok(())
}

pub fn unlock(verbose: bool) -> Result<()> {
    let state = SystemState::collect(verbose)?;
    if state.session.is_none() {
        bail!("No active graphical session to unlock.");
    }
    crate::rdp::unlock_session(&state, verbose)?;
    println!("  {} Screen unlocked.", "✓".green());
    Ok(())
}

// ── hydradesk fix ─────────────────────────────────────────────────────────────

pub fn fix(verbose: bool) -> Result<()> {
    println!("{}", "HydraDesk Fix".bold().cyan());
    println!("{}", "─".repeat(55).dimmed());
    println!();

    let state = SystemState::collect(verbose)?;
    let mut fixed_any = false;

    // Fix 1: gnome-remote-desktop not installed
    if state.grdctl_path.is_none() {
        use crate::detect::PackageManager;
        println!("Attempting to install gnome-remote-desktop...");
        let pm = PackageManager::detect();
        let out = match pm {
            PackageManager::Apt => Some(crate::detect::run_root_cmd(
                "apt-get",
                &["install", "-y", "gnome-remote-desktop"],
                verbose,
            )),
            PackageManager::Dnf => Some(crate::detect::run_root_cmd(
                "dnf",
                &["install", "-y", "gnome-remote-desktop"],
                verbose,
            )),
            PackageManager::Unknown => None,
        };
        match out {
            Some(Ok(o)) if o.success => {
                println!("  {} gnome-remote-desktop installed", "✓".green());
                fixed_any = true;
            }
            Some(Ok(o)) => println!("  {} package install failed:\n{}", "✗".red(), o.stderr),
            Some(Err(e)) => println!("  {} package manager not available: {}", "✗".red(), e),
            None => println!(
                "  {} No supported package manager (apt/dnf) found — install gnome-remote-desktop manually.",
                "✗".red()
            ),
        }
    }

    // Fix 2: Service not running
    if state.rdp_enabled && !state.rdp_port_open {
        println!("Attempting to restart gnome-remote-desktop service...");
        let out = crate::detect::run_cmd(
            "systemctl",
            &["--user", "restart", "gnome-remote-desktop.service"],
            &state.session,
            verbose,
        );
        match out {
            Ok(o) if o.success => {
                println!("  {} Service restarted", "✓".green());
                fixed_any = true;
            }
            Ok(o) => println!("  {} Restart failed:\n{}", "✗".red(), o.stderr),
            Err(e) => println!("  {} Could not restart: {}", "✗".red(), e),
        }
    }

    // Fix 3: Firewall blocking 3389
    if state.rdp_port_open {
        println!("Attempting to open port 3389 in firewall...");
        if let Err(e) = open_firewall_port(verbose) {
            println!("  {} {}", "✗".red(), e);
        } else {
            println!("  {} Port 3389 opened", "✓".green());
            fixed_any = true;
        }
    }

    if !fixed_any {
        println!("No automatic fixes were applicable.");
        println!("Run {} for full diagnostics.", "hydradesk doctor".bold());
    } else {
        println!();
        println!("Re-checking...");
        let _ = crate::doctor::run(verbose);
    }

    Ok(())
}

// ── hydradesk uninstall ─────────────────────────────────────────────────────

pub fn uninstall(verbose: bool) -> Result<()> {
    use crate::detect::{run_cmd, run_root_cmd};

    if !is_root() {
        bail!("hydradesk uninstall must be run as root.  Try:  sudo hydradesk uninstall");
    }

    banner();
    println!("{}", "HydraDesk Uninstall".bold().cyan());
    println!("{}", "─".repeat(55).dimmed());
    println!();

    let state = SystemState::collect(verbose).ok();

    let username = state
        .as_ref()
        .and_then(|s| s.session.as_ref().map(|x| x.username.clone()))
        .or_else(|| std::env::var("SUDO_USER").ok())
        .or_else(|| std::env::var("USER").ok())
        .unwrap_or_else(|| "user".to_string());
    let home = format!("/home/{}", username);

    println!("  Stopping virtual display service...");
    let _ = run_root_cmd("systemctl", &["stop", "hydradesk-display.service"], verbose);
    let _ = run_root_cmd("systemctl", &["disable", "hydradesk-display.service"], verbose);

    if let Some(st) = &state {
        if st.session.is_some() {
            println!("  Disabling GNOME Remote Desktop...");
            let grdctl = st.grdctl_path.as_deref().unwrap_or("/usr/bin/grdctl");
            let _ = run_cmd(grdctl, &["rdp", "disable"], &st.session, verbose);
            // disable --now so it does not auto-start again after a reboot
            let _ = run_cmd(
                "systemctl",
                &["--user", "disable", "--now", "gnome-remote-desktop.service"],
                &st.session,
                verbose,
            );
        }
    }

    println!("  Removing installed files...");
    for p in [
        "/usr/local/bin/hydradesk-display-init",
        "/etc/systemd/system/hydradesk-display.service",
    ] {
        let _ = std::fs::remove_file(p);
    }
    let _ = std::fs::remove_dir_all("/usr/local/share/hydradesk");
    let _ = std::fs::remove_file(format!("{}/.local/bin/hydradesk-rdp-init", home));
    let _ = std::fs::remove_file(format!("{}/.config/autostart/hydradesk-rdp.desktop", home));
    let _ = std::fs::remove_file(format!("{}/.local/bin/hydradesk-lock-watch", home));
    let _ = std::fs::remove_file(format!("{}/.config/autostart/hydradesk-lock.desktop", home));
    let _ = run_root_cmd("systemctl", &["daemon-reload"], verbose);

    let _ = std::fs::remove_file("/usr/local/bin/hydradesk");

    println!();
    println!("  {} HydraDesk removed.", "✓".green());
    println!();
    let pm = crate::detect::PackageManager::detect();
    println!("{}", "Left in place on purpose:".bold());
    println!(
        "  • GDM auto-login — disable in {} (AutomaticLoginEnable=false)",
        crate::detect::gdm_custom_conf()
    );
    println!("    if you no longer want it (skip on a headless box you still reach another way).");
    println!(
        "  • gnome-remote-desktop package — remove with: {} gnome-remote-desktop",
        pm.remove_hint()
    );
    Ok(())
}
