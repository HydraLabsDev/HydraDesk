// ==========================================================================
// Copyright (c) 2026 HydraCodeLabs
// Owner: HydraCodeLabs
// Project: HydraDesk
// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Last updated: 2026-06-27T01:32:17Z
// ==========================================================================

use colored::Colorize;

pub const BANNER: &str = r#"
██╗  ██╗██╗   ██╗██████╗ ██████╗  █████╗    ██████╗ ███████╗███████╗██╗  ██╗
██║  ██║╚██╗ ██╔╝██╔══██╗██╔══██╗██╔══██╗   ██╔══██╗██╔════╝██╔════╝██║ ██╔╝
███████║ ╚████╔╝ ██║  ██║██████╔╝███████║   ██║  ██║█████╗  ███████╗█████╔╝
██╔══██║  ╚██╔╝  ██║  ██║██╔══██╗██╔══██║   ██║  ██║██╔══╝  ╚════██║██╔═██╗
██║  ██║   ██║   ██████╔╝██║  ██║██║  ██║   ██████╔╝███████╗███████║██║  ██╗
╚═╝  ╚═╝   ╚═╝   ╚═════╝ ╚═╝  ╚═╝╚═╝  ╚═╝   ╚═════╝ ╚══════╝╚══════╝╚═╝  ╚═╝
"#;

/// Print the HydraDesk wordmark. Shown at the start of setup/uninstall.
pub fn banner() {
    println!("{}", BANNER.cyan().bold());
    println!(
        "{}",
        "        Physical Linux desktops over native Windows RDP".dimmed()
    );
    println!();
}

pub fn section(title: &str) {
    println!();
    println!("{}", title.bold().underline());
}

pub fn pass(msg: &str) {
    println!("  {} {}", "[PASS]".green().bold(), msg);
}

pub fn warn(msg: &str, reason: &str, fix: Option<&str>) {
    println!("  {} {}", "[WARN]".yellow().bold(), msg);
    println!("       {}", reason.dimmed());
    if let Some(f) = fix {
        println!("       {} {}", "Fix:".yellow(), f);
    }
}

pub fn fail(msg: &str, reason: &str, fix: Option<&str>) {
    println!("  {} {}", "[FAIL]".red().bold(), msg);
    println!("       {}", reason.dimmed());
    if let Some(f) = fix {
        println!("       {} {}", "Fix:".cyan(), f);
    }
}

pub fn info(msg: &str) {
    println!("  {} {}", "[INFO]".blue().bold(), msg);
}
