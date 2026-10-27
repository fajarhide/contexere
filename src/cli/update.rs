use colored::*;
use std::io::{self, Write};
#[cfg(not(target_family = "windows"))]
use std::process::Command;

pub fn print_help() {
    println!(
        "\n{} {}: Upgrade VENG to latest version",
        "veng".bold().cyan(),
        "update".bold().yellow()
    );
    println!("\n{}", "USAGE:".bold().bright_white());
    println!("  veng {}", "update".cyan());

    println!("\n{}", "DESCRIPTION:".bold().bright_white());
    println!("  Fetches the latest version from GitHub and upgrades VENG.");
    println!("  Currently supports Homebrew-based installations.");
    println!();
}

/// `veng update` takes no flags of its own; the guard is what stops a typo from
/// running the real update anyway and reporting success (#151).
const FLAGS: super::Flags = &[];

pub fn run(args: &[String]) -> Result<(), String> {
    if super::wants_help(args) {
        print_help();
        return Ok(());
    }
    super::check_flags("update", args, FLAGS).map_err(|e| e.to_string())?;

    let status = crate::guard::update::get_status();

    match status {
        crate::guard::update::Status::UpdateAvailable(latest_ver) => {
            println!(
                "\n{} A new version is available: {} → {}",
                "✨".yellow(),
                env!("CARGO_PKG_VERSION").bright_black(),
                latest_ver.green().bold()
            );

            print!(
                "   Confirm update to version {}? [y/N]: ",
                latest_ver.green().bold()
            );
            io::stdout().flush().unwrap();

            let mut input = String::new();
            if io::stdin().read_line(&mut input).is_err() {
                return Err("Failed to read input".to_string());
            }

            let input = input.trim().to_lowercase();
            if input != "y" && input != "yes" {
                println!("Update aborted.");
                return Ok(());
            }

            #[cfg(target_family = "windows")]
            {
                println!(
                    "\n{} Due to Windows installation methods, auto-update is not supported.",
                    "ℹ️".blue()
                );
                println!(
                    "   Please download the latest release from: https://github.com/fajarhide/veng/releases"
                );
                return Ok(());
            }

            #[cfg(not(target_family = "windows"))]
            {
                println!("{} Updating VENG via Homebrew...", "🚀".cyan());

                // Run brew upgrade
                let status = Command::new("brew")
                    .args(["upgrade", "fajarhide/tap/veng"])
                    .status();

                match status {
                    Ok(s) if s.success() => {
                        println!("\n{} VENG updated successfully!", "✓".green());
                    }
                    Ok(s) => {
                        return Err(format!(
                            "Brew upgrade failed with exit code: {}. You may need to run 'brew update' first.",
                            s.code().unwrap_or(1)
                        ));
                    }
                    Err(e) => {
                        return Err(format!(
                            "Failed to execute 'brew': {}. Please run 'brew upgrade fajarhide/tap/veng' manually.",
                            e
                        ));
                    }
                }
            }
        }
        crate::guard::update::Status::Ahead => {
            println!(
                "\n{} VENG is ahead of the latest stable release (v{}).",
                "🚀".blue(),
                env!("CARGO_PKG_VERSION")
            );
            println!("   You are currently on a pre-release or development version.");
        }
        crate::guard::update::Status::Latest => {
            println!(
                "\n{} VENG is already up to date (v{}).",
                "✓".green(),
                env!("CARGO_PKG_VERSION")
            );
        }
    }

    Ok(())
}
