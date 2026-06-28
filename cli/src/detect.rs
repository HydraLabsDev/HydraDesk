// ==========================================================================
// Copyright (c) 2026 HydraCodeLabs
// Owner: HydraCodeLabs
// Project: HydraDesk
// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Last updated: 2026-06-27T01:32:17Z
// ==========================================================================

use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum DesktopEnvironment {
    Gnome,
    Kde,
    Xfce,
    Lxde,
    Unknown(String),
}

impl std::fmt::Display for DesktopEnvironment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gnome => write!(f, "GNOME"),
            Self::Kde => write!(f, "KDE"),
            Self::Xfce => write!(f, "XFCE"),
            Self::Lxde => write!(f, "LXDE"),
            Self::Unknown(s) => write!(f, "Unknown ({})", s),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DisplayServer {
    Wayland,
    X11,
    Unknown,
}

impl std::fmt::Display for DisplayServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Wayland => write!(f, "Wayland"),
            Self::X11 => write!(f, "X11"),
            Self::Unknown => write!(f, "Unknown"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct OsInfo {
    pub name: String,
    pub version_id: String,
}

impl std::fmt::Display for OsInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.name, self.version_id)
    }
}

/// Context needed to run commands in the correct user session.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub session_id: String,
    pub username: String,
    pub uid: u32,
    pub display_server: DisplayServer,
    pub display: Option<String>,
    pub xauthority: Option<String>,
    /// unix:path=/run/user/<uid>/bus
    pub dbus_address: String,
    pub xdg_runtime_dir: String,
}

/// Full system snapshot collected before any action is taken.
#[derive(Debug, Clone)]
pub struct SystemState {
    pub os: OsInfo,
    pub desktop: DesktopEnvironment,
    pub display_server: DisplayServer,
    /// The active graphical session found via loginctl, if any.
    pub session: Option<SessionInfo>,
    pub has_monitor: bool,
    pub grdctl_path: Option<String>,
    pub rdp_enabled: bool,
    pub rdp_port_open: bool,
    pub local_ip: Option<String>,
}

pub struct CmdOutput {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
}

// ── Public helpers ────────────────────────────────────────────────────────────

/// Run a command as root (or current user if already root).
pub fn run_root_cmd(cmd: &str, args: &[&str], verbose: bool) -> Result<CmdOutput> {
    run_cmd_inner(cmd, args, None, verbose)
}

/// Run a command in the context of the graphical user's session.
/// If we are already that user, runs directly. If we are root, uses `runuser`.
pub fn run_cmd(
    cmd: &str,
    args: &[&str],
    session: &Option<SessionInfo>,
    verbose: bool,
) -> Result<CmdOutput> {
    if let Some(s) = session {
        let is_root = unsafe { libc::geteuid() } == 0;
        if is_root {
            // Use runuser to switch to the graphical user
            let dbus_env = format!("DBUS_SESSION_BUS_ADDRESS={}", s.dbus_address);
            let runtime_env = format!("XDG_RUNTIME_DIR={}", s.xdg_runtime_dir);
            let full_args: Vec<&str> = [
                "-u", &s.username, "--",
                "env",
                &dbus_env,
                &runtime_env,
                cmd,
            ]
            .iter()
            .copied()
            .chain(args.iter().copied())
            .collect();

            run_cmd_inner("runuser", &full_args, None, verbose)
        } else {
            run_cmd_inner(cmd, args, Some(s), verbose)
        }
    } else {
        run_cmd_inner(cmd, args, None, verbose)
    }
}

// ── Core runner ───────────────────────────────────────────────────────────────

pub fn run_cmd_inner(
    cmd: &str,
    args: &[&str],
    session: Option<&SessionInfo>,
    verbose: bool,
) -> Result<CmdOutput> {
    if verbose {
        eprintln!("[cmd] {} {}", cmd, args.join(" "));
    }

    let mut command = Command::new(cmd);
    command.args(args);

    if let Some(s) = session {
        command.env("DBUS_SESSION_BUS_ADDRESS", &s.dbus_address);
        command.env("XDG_RUNTIME_DIR", &s.xdg_runtime_dir);
        if let Some(d) = &s.display {
            command.env("DISPLAY", d);
        }
        if let Some(x) = &s.xauthority {
            command.env("XAUTHORITY", x);
        }
    }

    let output = command
        .output()
        .with_context(|| format!("Failed to execute: {}", cmd))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if verbose {
        if !stdout.trim().is_empty() {
            eprintln!("[stdout] {}", stdout.trim());
        }
        if !stderr.trim().is_empty() {
            eprintln!("[stderr] {}", stderr.trim());
        }
    }

    Ok(CmdOutput {
        stdout,
        stderr,
        success: output.status.success(),
    })
}

// ── Detection implementations ─────────────────────────────────────────────────

impl SystemState {
    pub fn collect(verbose: bool) -> Result<Self> {
        let os = detect_os()?;
        let session = detect_active_session(verbose)?;
        let desktop = detect_desktop(&session, verbose);
        let display_server = detect_display_server(&session, verbose);
        let has_monitor = detect_monitor(&session, verbose);
        let grdctl_path = find_grdctl();
        let rdp_enabled = check_rdp_enabled(&session, &grdctl_path, verbose);
        let rdp_port_open = check_port_3389(verbose);
        let local_ip = get_local_ip();

        Ok(SystemState {
            os,
            desktop,
            display_server,
            session,
            has_monitor,
            grdctl_path,
            rdp_enabled,
            rdp_port_open,
            local_ip,
        })
    }
}

// ── OS Detection ──────────────────────────────────────────────────────────────

pub fn detect_os() -> Result<OsInfo> {
    let content = std::fs::read_to_string("/etc/os-release")
        .context("Could not read /etc/os-release")?;

    let mut map = HashMap::new();
    for line in content.lines() {
        if let Some((k, v)) = line.split_once('=') {
            map.insert(k.trim().to_string(), v.trim().trim_matches('"').to_string());
        }
    }

    Ok(OsInfo {
        name: map.get("NAME").cloned().unwrap_or_else(|| "Linux".to_string()),
        version_id: map.get("VERSION_ID").cloned().unwrap_or_else(|| "?".to_string()),
    })
}

// ── Desktop Environment Detection ─────────────────────────────────────────────

pub fn detect_desktop(session: &Option<SessionInfo>, verbose: bool) -> DesktopEnvironment {
    // 1. Try XDG_CURRENT_DESKTOP from the process environment
    let from_env = std::env::var("XDG_CURRENT_DESKTOP")
        .or_else(|_| std::env::var("DESKTOP_SESSION"))
        .ok();

    if let Some(de) = &from_env {
        return parse_desktop_string(de);
    }

    // 2. Try reading from the session user's environment via /proc
    if let Some(s) = session {
        // Check /proc/<pid>/environ for the user's processes
        if let Ok(de) = read_de_from_proc(&s.username) {
            return de;
        }

        // 3. Fall back to process scan
        if let Ok(out) = run_cmd("ps", &["aux"], &Some(s.clone()), verbose) {
            let lower = out.stdout.to_lowercase();
            if lower.contains("gnome-shell") || lower.contains("gnome-session") {
                return DesktopEnvironment::Gnome;
            }
            if lower.contains("plasmashell") || lower.contains("kde") {
                return DesktopEnvironment::Kde;
            }
            if lower.contains("xfce4-session") || lower.contains("xfwm4") {
                return DesktopEnvironment::Xfce;
            }
            if lower.contains("lxsession") || lower.contains("lxde") {
                return DesktopEnvironment::Lxde;
            }
        }
    }

    // 4. Global process scan (when not running in session context)
    if let Ok(out) = run_root_cmd("ps", &["aux"], verbose) {
        let lower = out.stdout.to_lowercase();
        if lower.contains("gnome-shell") {
            return DesktopEnvironment::Gnome;
        }
        if lower.contains("plasmashell") {
            return DesktopEnvironment::Kde;
        }
        if lower.contains("xfce4-session") {
            return DesktopEnvironment::Xfce;
        }
    }

    DesktopEnvironment::Unknown(String::new())
}

fn parse_desktop_string(s: &str) -> DesktopEnvironment {
    let lower = s.to_lowercase();
    if lower.contains("gnome") {
        DesktopEnvironment::Gnome
    } else if lower.contains("kde") || lower.contains("plasma") {
        DesktopEnvironment::Kde
    } else if lower.contains("xfce") {
        DesktopEnvironment::Xfce
    } else if lower.contains("lxde") || lower.contains("lxqt") {
        DesktopEnvironment::Lxde
    } else {
        DesktopEnvironment::Unknown(s.to_string())
    }
}

fn read_de_from_proc(_username: &str) -> Result<DesktopEnvironment> {
    // Iterate /proc/<pid>/environ for the user's gnome-shell process
    for entry in std::fs::read_dir("/proc")? {
        let entry = entry?;
        let name = entry.file_name();
        let pid_str = name.to_string_lossy();
        if !pid_str.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }

        let status_path = entry.path().join("status");
        let _cmdline_path = entry.path().join("cmdline");

        let Ok(status) = std::fs::read_to_string(&status_path) else { continue };
        // Check process belongs to user
        let belongs = status
            .lines()
            .any(|l| l.starts_with("Name:") && l.contains("gnome-shell"));
        if !belongs {
            continue;
        }

        let environ_path = entry.path().join("environ");
        let Ok(environ) = std::fs::read(&environ_path) else { continue };

        for var in environ.split(|&b| b == 0) {
            let s = String::from_utf8_lossy(var);
            if s.starts_with("XDG_CURRENT_DESKTOP=") {
                let val = s.trim_start_matches("XDG_CURRENT_DESKTOP=");
                return Ok(parse_desktop_string(val));
            }
        }
    }
    Err(anyhow!("not found"))
}

// ── Display Server Detection ──────────────────────────────────────────────────

pub fn detect_display_server(session: &Option<SessionInfo>, verbose: bool) -> DisplayServer {
    // If we have a session already, use its type
    if let Some(s) = session {
        return s.display_server.clone();
    }

    // Try env
    if let Ok(t) = std::env::var("XDG_SESSION_TYPE") {
        return parse_display_server(&t);
    }

    // Try loginctl
    if let Ok(out) = run_root_cmd("loginctl", &["list-sessions", "--no-legend"], verbose) {
        for line in out.stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 1 { continue; }
            let sid = parts[0];
            if let Ok(show) = run_root_cmd("loginctl", &["show-session", sid], verbose) {
                if show.stdout.contains("Active=yes") {
                    if let Some(t) = extract_field(&show.stdout, "Type") {
                        return parse_display_server(&t);
                    }
                }
            }
        }
    }

    DisplayServer::Unknown
}

fn parse_display_server(s: &str) -> DisplayServer {
    match s.to_lowercase().as_str() {
        "wayland" => DisplayServer::Wayland,
        "x11" | "mir" => DisplayServer::X11,
        _ => DisplayServer::Unknown,
    }
}

// ── Session Detection ─────────────────────────────────────────────────────────

pub fn detect_active_session(verbose: bool) -> Result<Option<SessionInfo>> {
    let out = run_root_cmd("loginctl", &["list-sessions", "--no-legend"], verbose)
        .context("loginctl not found — is systemd installed?")?;

    for line in out.stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        // SESSION  UID  USER  SEAT  TTY
        if parts.len() < 3 {
            continue;
        }
        let session_id = parts[0];
        let uid_str = parts[1];
        let username = parts[2];

        let uid: u32 = uid_str.parse().unwrap_or(0);
        if uid < 1000 {
            continue; // skip system accounts
        }

        let show = run_root_cmd("loginctl", &["show-session", session_id], verbose)?;

        let active = show.stdout.contains("Active=yes");
        let state_ok = extract_field(&show.stdout, "State")
            .map(|s| s == "active" || s == "online")
            .unwrap_or(false);

        if !active && !state_ok {
            continue;
        }

        let session_type_str = extract_field(&show.stdout, "Type").unwrap_or_default();
        // Skip pure TTY sessions
        if session_type_str == "tty" {
            continue;
        }

        let display_server = parse_display_server(&session_type_str);
        let xdg_runtime_dir = format!("/run/user/{}", uid);
        let dbus_address = format!("unix:path={}/bus", xdg_runtime_dir);

        // Find DISPLAY
        let display = std::env::var("DISPLAY").ok().or_else(|| {
            // Try to get it from the user's running processes
            get_display_from_proc(uid)
        });

        // Find XAUTHORITY
        let xauthority = std::env::var("XAUTHORITY").ok().or_else(|| {
            let candidates = [
                format!("{}/gdm/Xauthority", xdg_runtime_dir),
                format!("/home/{}/.Xauthority", username),
                format!("/root/.Xauthority"),
            ];
            candidates.into_iter().find(|p| Path::new(p).exists())
        });

        return Ok(Some(SessionInfo {
            session_id: session_id.to_string(),
            username: username.to_string(),
            uid,
            display_server,
            display,
            xauthority,
            dbus_address,
            xdg_runtime_dir,
        }));
    }

    Ok(None)
}

fn get_display_from_proc(uid: u32) -> Option<String> {
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let pid_str = entry.file_name();
        if !pid_str.to_string_lossy().chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        // Check UID
        let status = std::fs::read_to_string(entry.path().join("status")).ok()?;
        let proc_uid: u32 = status
            .lines()
            .find(|l| l.starts_with("Uid:"))?
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()?;
        if proc_uid != uid {
            continue;
        }
        // Read environ
        let environ = std::fs::read(entry.path().join("environ")).ok()?;
        for var in environ.split(|&b| b == 0) {
            let s = String::from_utf8_lossy(var);
            if s.starts_with("DISPLAY=") {
                return Some(s.trim_start_matches("DISPLAY=").to_string());
            }
        }
    }
    None
}

// ── Monitor Detection ─────────────────────────────────────────────────────────

pub fn detect_monitor(session: &Option<SessionInfo>, verbose: bool) -> bool {
    // Try xrandr in the user's session
    if let Ok(out) = run_cmd("xrandr", &["--query"], session, verbose) {
        if out.success {
            return out.stdout.contains(" connected ");
        }
    }

    // Fall back to /sys/class/drm
    if let Ok(dir) = std::fs::read_dir("/sys/class/drm") {
        for entry in dir.flatten() {
            let status_path = entry.path().join("status");
            if let Ok(s) = std::fs::read_to_string(status_path) {
                if s.trim() == "connected" {
                    return true;
                }
            }
        }
    }

    false
}

// ── grdctl ────────────────────────────────────────────────────────────────────

pub fn find_grdctl() -> Option<String> {
    let candidates = [
        "/usr/bin/grdctl",
        "/usr/local/bin/grdctl",
        "/snap/bin/grdctl",
    ];
    for c in &candidates {
        if Path::new(c).exists() {
            return Some(c.to_string());
        }
    }
    // Try which
    if let Ok(out) = Command::new("which").arg("grdctl").output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    None
}

pub fn check_rdp_enabled(
    session: &Option<SessionInfo>,
    grdctl_path: &Option<String>,
    verbose: bool,
) -> bool {
    let Some(grdctl) = grdctl_path else { return false };

    for args in [&["rdp", "status"][..], &["status"][..]] {
        let Ok(out) = run_cmd(grdctl, args, session, verbose) else {
            continue;
        };
        if let Some(enabled) = parse_grdctl_rdp_enabled(&out.stdout, &out.stderr) {
            return enabled;
        }
    }

    false
}

fn parse_grdctl_rdp_enabled(stdout: &str, stderr: &str) -> Option<bool> {
    let text = format!("{}\n{}", stdout, stderr);

    for raw in text.lines() {
        let line = raw.trim().to_lowercase();
        if line.is_empty() {
            continue;
        }

        let normalized = line
            .replace(':', " ")
            .replace('=', " ")
            .replace('\t', " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");

        if normalized.contains("not enabled")
            || normalized.contains("status disabled")
            || normalized.contains("status is disabled")
            || normalized.contains("enabled no")
            || normalized.contains("rdp disabled")
            || normalized.contains("rdp is disabled")
            || normalized == "disabled"
        {
            return Some(false);
        }

        if normalized.contains("status enabled")
            || normalized.contains("status is enabled")
            || normalized.contains("enabled yes")
            || normalized.contains("rdp enabled")
            || normalized.contains("rdp is enabled")
            || normalized == "enabled"
        {
            return Some(true);
        }
    }

    None
}

// ── Port Check ────────────────────────────────────────────────────────────────

pub fn check_port_3389(verbose: bool) -> bool {
    if let Ok(out) = run_root_cmd("ss", &["-tulnp"], verbose) {
        if out.stdout.contains(":3389") {
            return true;
        }
    }
    // Fallback: netstat
    if let Ok(out) = run_root_cmd("netstat", &["-tulnp"], verbose) {
        if out.stdout.contains(":3389") {
            return true;
        }
    }
    false
}

// ── Local IP ──────────────────────────────────────────────────────────────────

pub fn get_local_ip() -> Option<String> {
    // UDP trick: no packet is actually sent
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    socket.local_addr().ok().map(|a| a.ip().to_string())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Extract "KEY=value" → "value" from loginctl show-session output.
pub fn extract_field(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix(&format!("{}=", key)) {
            return Some(rest.trim().to_string());
        }
    }
    None
}

/// libc uid check for "are we root"
pub fn is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

// ── Platform / distro abstraction ───────────────────────────────────────────────
//
// HydraDesk targets GNOME on Wayland, which spans both Debian-family (apt, ufw,
// /etc/gdm3) and Red Hat-family (dnf, firewalld, /etc/gdm) distros. These helpers
// keep the rest of the code distro-agnostic: detection is by probing the live
// system (which binary exists, which directory is present) rather than parsing
// /etc/os-release, so derivatives are handled the same as their parents.

/// Is `name` an executable on PATH?
pub fn binary_exists(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageManager {
    /// Debian, Ubuntu, and derivatives.
    Apt,
    /// Fedora, RHEL, and derivatives (covers `yum` via the dnf-compatible CLI).
    Dnf,
    Unknown,
}

impl PackageManager {
    pub fn detect() -> Self {
        if binary_exists("apt-get") {
            PackageManager::Apt
        } else if binary_exists("dnf") || binary_exists("yum") {
            PackageManager::Dnf
        } else {
            PackageManager::Unknown
        }
    }

    /// The remove command a user would run by hand, e.g. "sudo apt remove".
    pub fn remove_hint(&self) -> &'static str {
        match self {
            PackageManager::Apt => "sudo apt remove",
            PackageManager::Dnf => "sudo dnf remove",
            PackageManager::Unknown => "your package manager's remove command",
        }
    }

    /// The install command a user would run by hand, e.g. "sudo apt install".
    pub fn install_hint(&self) -> &'static str {
        match self {
            PackageManager::Apt => "sudo apt install",
            PackageManager::Dnf => "sudo dnf install",
            PackageManager::Unknown => "your package manager's install command",
        }
    }
}

/// Path to GDM's daemon config. Debian/Ubuntu ship it under /etc/gdm3; Fedora,
/// RHEL, Arch, and openSUSE use /etc/gdm.
pub fn gdm_custom_conf() -> &'static str {
    if Path::new("/etc/gdm3").is_dir() {
        "/etc/gdm3/custom.conf"
    } else {
        "/etc/gdm/custom.conf"
    }
}
