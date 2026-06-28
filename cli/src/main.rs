// ==========================================================================
// Copyright (c) 2026 HydraCodeLabs
// Owner: HydraCodeLabs
// Project: HydraDesk
// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Last updated: 2026-06-27T01:32:17Z
// ==========================================================================

mod detect;
mod doctor;
mod output;
mod rdp;
mod setup;

use clap::{Parser, Subcommand};
use colored::Colorize;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "hydradesk",
    about = "Linux RDP setup — physical desktop via Windows mstsc",
    long_about = None,
    version,
)]
pub struct Cli {
    /// Print every command executed and its raw output
    #[arg(long, global = true)]
    pub debug: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Detect system state and configure GNOME Remote Desktop for mstsc access
    Setup {
        /// RDP password to set (prompted for if omitted)
        #[arg(long)]
        password: Option<String>,

        /// Read the RDP password from standard input instead of a command-line argument
        #[arg(long, conflicts_with_all = ["password", "password_file"])]
        password_stdin: bool,

        /// Read the RDP password from a local file instead of a command-line argument
        #[arg(long, conflicts_with_all = ["password", "password_stdin"])]
        password_file: Option<PathBuf>,

        /// RDP username (defaults to the logged-in user; prompted for if omitted)
        #[arg(long)]
        username: Option<String>,

        /// Skip firewall configuration (ufw / firewalld)
        #[arg(long)]
        no_firewall: bool,

        /// Minutes of no RDP client before the screen auto-locks (0 = never; prompted if omitted)
        #[arg(long)]
        lock_timeout: Option<u64>,
    },

    /// Lock the physical screen now
    Lock,

    /// Unlock the physical screen now
    Unlock,

    /// Remove HydraDesk's services, generated files, and the binary
    Uninstall,

    /// Run diagnostics and print each check with pass/fail/warn
    Doctor,

    /// Show current RDP status, connection details, and recent activity
    Status,

    /// Self-check the full connect path: session, framebuffer, RDP, port
    Test,

    /// Show GNOME Remote Desktop journal logs
    Logs {
        /// Number of recent log lines to print
        #[arg(long, default_value_t = 80)]
        lines: usize,

        /// Show logs since this journalctl time expression
        #[arg(long, default_value = "24 hours ago")]
        since: String,

        /// Follow new log lines
        #[arg(short, long)]
        follow: bool,
    },

    /// Attempt to fix the most recent detected issue
    Fix,

    /// Print full debug info: commands, output, logs, system state
    Debug,
}

fn main() {
    let cli = Cli::parse();

    let result = match &cli.command {
        Commands::Doctor => doctor::run(cli.debug),
        Commands::Setup {
            password,
            password_stdin,
            password_file,
            username,
            no_firewall,
            lock_timeout,
        } => {
            setup::run(
                password.clone(),
                *password_stdin,
                password_file.clone(),
                username.clone(),
                *no_firewall,
                *lock_timeout,
                cli.debug,
            )
        }
        Commands::Lock => setup::lock(cli.debug),
        Commands::Unlock => setup::unlock(cli.debug),
        Commands::Status => rdp::status(cli.debug),
        Commands::Test => rdp::test(cli.debug),
        Commands::Logs { lines, since, follow } => run_logs(*lines, since, *follow, cli.debug),
        Commands::Uninstall => setup::uninstall(cli.debug),
        Commands::Fix => setup::fix(cli.debug),
        Commands::Debug => run_debug(cli.debug),
    };

    if let Err(e) = result {
        eprintln!("{} {}", "error:".red().bold(), e);
        std::process::exit(1);
    }
}

fn run_logs(lines: usize, since: &str, follow: bool, verbose: bool) -> anyhow::Result<()> {
    use output::section;

    section("GNOME Remote Desktop Logs");
    let state = detect::SystemState::collect(verbose)?;

    let mut args = vec![
        "--user".to_string(),
        "-u".to_string(),
        "gnome-remote-desktop.service".to_string(),
        "--no-pager".to_string(),
    ];

    if follow {
        args.push("-f".to_string());
        return stream_logs(args, &state, verbose);
    } else {
        args.push("--since".to_string());
        args.push(since.to_string());
        args.push("-n".to_string());
        args.push(lines.to_string());
    }

    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = detect::run_cmd("journalctl", &arg_refs, &state.session, verbose)?;
    print!("{}", out.stdout);
    if !out.stderr.trim().is_empty() {
        eprint!("{}", out.stderr);
    }

    Ok(())
}

fn stream_logs(
    args: Vec<String>,
    state: &detect::SystemState,
    verbose: bool,
) -> anyhow::Result<()> {
    use anyhow::Context;
    use std::process::Command;

    if let Some(session) = &state.session {
        let is_root = unsafe { libc::geteuid() } == 0;
        if is_root {
            let dbus_env = format!("DBUS_SESSION_BUS_ADDRESS={}", session.dbus_address);
            let runtime_env = format!("XDG_RUNTIME_DIR={}", session.xdg_runtime_dir);
            let mut command = Command::new("runuser");
            command
                .arg("-u")
                .arg(&session.username)
                .arg("--")
                .arg("env")
                .arg(dbus_env)
                .arg(runtime_env)
                .arg("journalctl")
                .args(&args);

            if verbose {
                eprintln!("[cmd] runuser -u {} -- env ... journalctl {}", session.username, args.join(" "));
            }
            let status = command.status().context("Failed to execute: runuser")?;
            if !status.success() {
                anyhow::bail!("journalctl exited with status {}", status);
            }
            return Ok(());
        }

        let mut command = Command::new("journalctl");
        command
            .env("DBUS_SESSION_BUS_ADDRESS", &session.dbus_address)
            .env("XDG_RUNTIME_DIR", &session.xdg_runtime_dir)
            .args(&args);

        if let Some(display) = &session.display {
            command.env("DISPLAY", display);
        }
        if let Some(xauthority) = &session.xauthority {
            command.env("XAUTHORITY", xauthority);
        }

        if verbose {
            eprintln!("[cmd] journalctl {}", args.join(" "));
        }
        let status = command.status().context("Failed to execute: journalctl")?;
        if !status.success() {
            anyhow::bail!("journalctl exited with status {}", status);
        }
        return Ok(());
    }

    if verbose {
        eprintln!("[cmd] journalctl {}", args.join(" "));
    }
    let status = Command::new("journalctl")
        .args(&args)
        .status()
        .context("Failed to execute: journalctl")?;
    if !status.success() {
        anyhow::bail!("journalctl exited with status {}", status);
    }
    Ok(())
}

fn run_debug(verbose: bool) -> anyhow::Result<()> {
    use output::section;

    section("System Detection");
    let state = detect::SystemState::collect(verbose)?;
    println!("{:#?}", state);

    section("GNOME Remote Desktop Journal");
    let out = detect::run_cmd(
        "journalctl",
        &["--user", "-u", "gnome-remote-desktop.service", "-n", "50", "--no-pager"],
        &state.session,
        verbose,
    );
    match out {
        Ok(o) => println!("{}", o.stdout),
        Err(e) => println!("(not available: {})", e),
    }

    section("Open Ports");
    if let Ok(o) = detect::run_root_cmd("ss", &["-tulnp"], verbose) {
        println!("{}", o.stdout);
    }

    section("Firewall");
    let firewalld_running = detect::run_root_cmd("firewall-cmd", &["--state"], verbose)
        .map(|o| o.success && o.stdout.trim() == "running")
        .unwrap_or(false);
    if firewalld_running {
        if let Ok(o) = detect::run_root_cmd("firewall-cmd", &["--list-all"], verbose) {
            println!("{}", o.stdout);
        }
    } else if let Ok(o) = detect::run_root_cmd("ufw", &["status", "verbose"], verbose) {
        println!("{}", o.stdout);
    }

    Ok(())
}
