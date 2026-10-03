// SPDX-FileCopyrightText: Hadi Cherkaoui <contact@hide.cherkaoui.ch>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fs;
use std::path::{Path, PathBuf};
use std::process;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::*;

use sd2dinit::config::{Config, config_home};
use sd2dinit::converter;
use sd2dinit::generator;
use sd2dinit::hook::is_user_unit;
use sd2dinit::model::Severity;
use sd2dinit::parser::SystemdUnit;
use sd2dinit::services::KnownServices;
/// Attribution shown by `--help`, and by `--version` alongside the version.
///
/// The binary carries the credit and the source offer itself, so both survive
/// being repackaged, vendored, or shipped without the README. ASCII only --
/// this has to render on a Windows console at a legacy code page too.
const CREDIT: &str = concat!(
    "Copyright (C) Hadi Cherkaoui\n",
    "Licence: AGPL-3.0-or-later\n",
    "Source:  ",
    env!("CARGO_PKG_REPOSITORY"),
);

const LONG_VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    "\n\nCopyright (C) Hadi Cherkaoui\nLicence: AGPL-3.0-or-later\nSource:  ",
    env!("CARGO_PKG_REPOSITORY"),
);

#[derive(Parser)]
#[command(
    name = "sd2dinit",
    about = "Convert systemd unit files to dinit service files",
    version,
    long_version = LONG_VERSION,
    after_help = CREDIT
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Convert a systemd unit file to dinit format
    Convert {
        /// Path to the systemd .service unit file
        unit_file: PathBuf,
        /// Output directory for generated dinit files
        #[arg(long)]
        output_dir: Option<PathBuf>,
        /// Print generated files without writing
        #[arg(long)]
        dry_run: bool,
        /// Overwrite existing dinit files
        #[arg(long)]
        force: bool,
    },
    /// Convert and optionally enable/start the service via dinitctl
    Install {
        /// Path to the systemd .service unit file
        unit_file: PathBuf,
        /// Output directory for generated dinit files
        #[arg(long)]
        output_dir: Option<PathBuf>,
        /// Enable the service via dinitctl
        #[arg(long)]
        enable: bool,
        /// Start the service via dinitctl
        #[arg(long)]
        start: bool,
        /// Print generated files without writing
        #[arg(long)]
        dry_run: bool,
        /// Overwrite existing dinit files
        #[arg(long)]
        force: bool,
    },
    /// Pacman hook mode — reads target paths from stdin and batch converts
    Hook,
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Convert {
            unit_file,
            output_dir,
            dry_run,
            force,
        } => run_convert(&unit_file, output_dir.as_deref(), dry_run, force)
            .map(|converted| converted.exit_code),
        Commands::Install {
            unit_file,
            output_dir,
            enable,
            start,
            dry_run,
            force,
        } => run_install(
            &unit_file,
            output_dir.as_deref(),
            enable,
            start,
            dry_run,
            force,
        ),
        Commands::Hook => run_hook(),
    };

    match result {
        Ok(exit_code) => process::exit(exit_code),
        Err(e) => {
            eprintln!("{} {:#}", "error:".red().bold(), e);
            process::exit(2);
        }
    }
}

/// What [`run_convert`] produced, for `install` to act on.
struct Converted {
    exit_code: i32,
    /// The main service, then its `-post` helper, which dinit does not start by itself.
    services: Vec<String>,
    user: bool,
}

impl Converted {
    fn skipped(user: bool) -> Self {
        Self {
            exit_code: 1,
            services: Vec::new(),
            user,
        }
    }
}

fn run_convert(
    unit_file: &Path,
    output_dir: Option<&Path>,
    dry_run: bool,
    force: bool,
) -> Result<Converted> {
    let mut config = Config::load().context("failed to load config")?;

    // The converter derives script and env-file paths from output_dir, so it must be final here
    let user = is_user_unit(unit_file);
    if let Some(dir) = output_dir {
        config.output_dir = dir.to_path_buf();
    } else if user {
        config.output_dir = default_user_output_dir(unit_file, &config)?;
    }
    // dinit resolves relative paths against the service file's directory, not ours
    config.output_dir = std::path::absolute(&config.output_dir)
        .context("failed to resolve the output directory")?;

    // Reject non-.service files (including extension-less files)
    match unit_file.extension().and_then(|e| e.to_str()) {
        Some("service") => {}
        Some(ext) => {
            eprintln!(
                "{} only .service units are supported, got .{} — skipping {}",
                "warning:".yellow().bold(),
                ext,
                unit_file.display()
            );
            return Ok(Converted::skipped(user));
        }
        None => {
            eprintln!(
                "{} only .service units are supported — skipping {}",
                "warning:".yellow().bold(),
                unit_file.display()
            );
            return Ok(Converted::skipped(user));
        }
    }

    // Reject template/instance units
    if let Some(stem) = unit_file.file_stem().and_then(|s| s.to_str())
        && stem.contains('@')
    {
        eprintln!(
            "{} template/instance units not supported — skipping {}",
            "warning:".yellow().bold(),
            unit_file.display()
        );
        return Ok(Converted::skipped(user));
    }

    let unit = SystemdUnit::load(unit_file)
        .with_context(|| format!("failed to load {}", unit_file.display()))?;

    let mut service_dirs = if user {
        config.user_service_dirs.clone()
    } else {
        config.service_dirs.clone()
    };
    let own_dirs = own_user_service_dirs();
    // A shared user service must not depend on one that only the caller has
    if user && own_dirs.contains(&config.output_dir) {
        service_dirs.extend(own_dirs);
    }
    service_dirs.push(config.output_dir.clone());
    let known = KnownServices::scan(&service_dirs);

    let result = converter::convert(&unit, &config, &known)
        .with_context(|| format!("failed to convert {}", unit_file.display()))?;

    let mut had_warnings = false;

    for w in &result.warnings {
        had_warnings = true;
        let prefix = match w.severity {
            Severity::Info => "info:".blue().bold(),
            Severity::Warn => "warning:".yellow().bold(),
            Severity::Error => "error:".red().bold(),
        };
        eprintln!("{} [{}] {}", prefix, w.directive, w.message);
    }

    // Write or print all generated artifacts
    write_or_print(
        &config.output_dir.join(&result.main_service.name),
        &generator::generate(&result.main_service),
        dry_run,
        force,
        &result.main_service.name,
    )?;

    if let Some(ref pre) = result.pre_service {
        write_or_print(
            &config.output_dir.join(&pre.name),
            &generator::generate(pre),
            dry_run,
            force,
            &pre.name,
        )?;
    }

    if let Some(ref post) = result.post_service {
        write_or_print(
            &config.output_dir.join(&post.name),
            &generator::generate(post),
            dry_run,
            force,
            &post.name,
        )?;
    }

    if let Some(ref script) = result.pre_script {
        let name = format!("{}-pre.sh", result.main_service.name);
        write_or_print(
            &config.output_dir.join(&name),
            script,
            dry_run,
            force,
            &name,
        )?;
    }
    if let Some(ref script) = result.post_script {
        let name = format!("{}-post.sh", result.main_service.name);
        write_or_print(
            &config.output_dir.join(&name),
            script,
            dry_run,
            force,
            &name,
        )?;
    }
    if let Some(ref script) = result.start_script {
        let name = format!("{}-start.sh", result.main_service.name);
        write_or_print(
            &config.output_dir.join(&name),
            script,
            dry_run,
            force,
            &name,
        )?;
    }
    if let Some(ref script) = result.stop_script {
        let name = format!("{}-stop.sh", result.main_service.name);
        write_or_print(
            &config.output_dir.join(&name),
            script,
            dry_run,
            force,
            &name,
        )?;
    }

    if let Some(ref env_content) = result.env_file_content {
        let name = format!("{}.env", result.main_service.name);
        write_or_print(
            &config.output_dir.join(&name),
            env_content,
            dry_run,
            force,
            &name,
        )?;
    }

    if !dry_run {
        eprintln!(
            "{} {} → {}",
            "converted:".green().bold(),
            unit_file.display(),
            config.output_dir.join(&result.main_service.name).display()
        );
    }

    let mut services = vec![result.main_service.name.clone()];
    services.extend(result.post_service.map(|post| post.name));
    Ok(Converted {
        exit_code: if had_warnings { 1 } else { 0 },
        services,
        user,
    })
}

/// Where a user unit goes when no `--output-dir` is given.
///
/// A unit from the caller's own `~/.config/systemd/user` goes to their own
/// `~/.config/dinit.d`; any other goes to the shared `user_output_dir`.
///
/// # Errors
///
/// Fails for a unit from another user's `~/.config/systemd/user`, which must
/// not land where every user's dinit loads it.
fn default_user_output_dir(unit_file: &Path, config: &Config) -> Result<PathBuf> {
    // Canonical on both sides, so a symlinked ~/.config still matches
    let canonical = |path: &Path| fs::canonicalize(path).or_else(|_| std::path::absolute(path));
    let unit = canonical(unit_file)
        .with_context(|| format!("failed to resolve {}", unit_file.display()))?;
    if let Some(home) = config_home()
        && canonical(&home).is_ok_and(|real| unit.starts_with(real.join("systemd/user")))
    {
        return Ok(home.join("dinit.d"));
    }
    let names: Vec<_> = unit.iter().collect();
    if names
        .windows(3)
        .any(|w| w[0] == ".config" && w[1] == "systemd" && w[2] == "user")
    {
        anyhow::bail!(
            "{} is another user's unit — pass --output-dir to choose where it goes",
            unit_file.display()
        );
    }
    Ok(config.user_output_dir.clone())
}

/// The calling user's own dinit service directories, as a user dinit instance searches them.
fn own_user_service_dirs() -> Vec<PathBuf> {
    let var = |name| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let mut dirs: Vec<PathBuf> = var("XDG_CONFIG_HOME").into_iter().collect();
    dirs.extend(var("HOME").map(|home| home.join(".config")));
    dirs.into_iter().map(|dir| dir.join("dinit.d")).collect()
}

fn run_install(
    unit_file: &Path,
    output_dir: Option<&Path>,
    enable: bool,
    start: bool,
    dry_run: bool,
    force: bool,
) -> Result<i32> {
    let converted = run_convert(unit_file, output_dir, dry_run, force)?;
    // Without --user, dinitctl run as root talks to the system instance
    let scope = if converted.user { "--user " } else { "" };

    for (action, done, wanted) in [("enable", "enabled", enable), ("start", "started", start)] {
        if !wanted {
            continue;
        }
        for service in &converted.services {
            if dry_run {
                eprintln!(
                    "{} would run: dinitctl {scope}{action} {service}",
                    "dry-run:".cyan().bold()
                );
                continue;
            }
            let mut dinitctl = process::Command::new("dinitctl");
            if converted.user {
                dinitctl.arg("--user");
            }
            let status = dinitctl
                .args([action, service])
                .status()
                .with_context(|| format!("failed to run dinitctl {action}"))?;
            if !status.success() {
                eprintln!(
                    "{} dinitctl {scope}{action} {service} failed",
                    "error:".red().bold()
                );
                if converted.user {
                    eprintln!(
                        "{} user services belong to each user's dinit: run `dinitctl {action} {service}` as that user",
                        "hint:".cyan().bold()
                    );
                }
                return Ok(2);
            }
            eprintln!("{} {done} {service}", "ok:".green().bold());
        }
    }

    Ok(converted.exit_code)
}

fn run_hook() -> Result<i32> {
    let config = Config::load().context("failed to load config")?;
    sd2dinit::hook::run_hook(&config)?;
    Ok(0)
}

fn write_or_print(
    path: &Path,
    content: &str,
    dry_run: bool,
    force: bool,
    label: &str,
) -> Result<()> {
    if dry_run {
        println!("\n--- {} ---", label.bold());
        println!("{}", content);
        return Ok(());
    }

    if path.exists() && !force {
        eprintln!(
            "{} {} already exists — use --force to overwrite",
            "skip:".yellow().bold(),
            path.display()
        );
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }

    fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))?;

    // Make shell scripts executable on Unix
    #[cfg(unix)]
    if path.extension().map(|e| e == "sh").unwrap_or(false) {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .with_context(|| format!("failed to chmod {}", path.display()))?;
    }

    Ok(())
}
