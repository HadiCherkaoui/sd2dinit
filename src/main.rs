use std::fs;
use std::path::{Path, PathBuf};
use std::process;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::*;

use sd2dinit::config::Config;
use sd2dinit::converter;
use sd2dinit::generator;
use sd2dinit::model::Severity;
use sd2dinit::parser::SystemdUnit;

#[derive(Parser)]
#[command(
    name = "sd2dinit",
    about = "Convert systemd unit files to dinit service files",
    version
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
        Commands::Convert { unit_file, output_dir, dry_run, force } => {
            run_convert(&unit_file, output_dir.as_deref(), dry_run, force)
        }
        Commands::Install { unit_file, output_dir, enable, start, dry_run, force } => {
            run_install(&unit_file, output_dir.as_deref(), enable, start, dry_run, force)
        }
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

fn run_convert(
    unit_file: &PathBuf,
    output_dir: Option<&Path>,
    dry_run: bool,
    force: bool,
) -> Result<i32> {
    let mut config = Config::load().context("failed to load config")?;

    // CLI --output-dir overrides config (so converter uses correct script paths)
    if let Some(dir) = output_dir {
        config.output_dir = dir.to_path_buf();
    }

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
            return Ok(1);
        }
        None => {
            eprintln!(
                "{} only .service units are supported — skipping {}",
                "warning:".yellow().bold(),
                unit_file.display()
            );
            return Ok(1);
        }
    }

    // Reject template/instance units
    if let Some(stem) = unit_file.file_stem().and_then(|s| s.to_str()) {
        if stem.contains('@') {
            eprintln!(
                "{} template/instance units not supported — skipping {}",
                "warning:".yellow().bold(),
                unit_file.display()
            );
            return Ok(1);
        }
    }

    let content = fs::read_to_string(unit_file)
        .with_context(|| format!("failed to read {}", unit_file.display()))?;

    let mut unit = SystemdUnit::parse(&content, unit_file.clone())
        .with_context(|| format!("failed to parse {}", unit_file.display()))?;

    // Apply drop-in overrides from <unit>.d/*.conf
    let drop_in_dir = {
        let mut p = unit_file.to_path_buf();
        let name = unit_file.file_name().unwrap_or_default().to_string_lossy().into_owned();
        p.set_file_name(format!("{}.d", name));
        p
    };
    if drop_in_dir.is_dir() {
        let mut entries: Vec<_> = fs::read_dir(&drop_in_dir)
            .with_context(|| format!("failed to read drop-in dir {}", drop_in_dir.display()))?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|x| x == "conf").unwrap_or(false))
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let drop_content = fs::read_to_string(entry.path())
                .with_context(|| format!("failed to read drop-in {}", entry.path().display()))?;
            unit.merge_drop_in(&drop_content, entry.path());
        }
    }

    let result = converter::convert(&unit, &config)
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
        write_or_print(&config.output_dir.join(&name), script, dry_run, force, &name)?;
    }
    if let Some(ref script) = result.post_script {
        let name = format!("{}-post.sh", result.main_service.name);
        write_or_print(&config.output_dir.join(&name), script, dry_run, force, &name)?;
    }
    if let Some(ref script) = result.stop_script {
        let name = format!("{}-stop.sh", result.main_service.name);
        write_or_print(&config.output_dir.join(&name), script, dry_run, force, &name)?;
    }

    if let Some(ref env_content) = result.env_file_content {
        let name = format!("{}.env", result.main_service.name);
        write_or_print(&config.output_dir.join(&name), env_content, dry_run, force, &name)?;
    }

    if !dry_run {
        eprintln!(
            "{} {} → {}",
            "converted:".green().bold(),
            unit_file.display(),
            config.output_dir.join(&result.main_service.name).display()
        );
    }

    Ok(if had_warnings { 1 } else { 0 })
}

fn run_install(
    unit_file: &PathBuf,
    output_dir: Option<&Path>,
    enable: bool,
    start: bool,
    dry_run: bool,
    force: bool,
) -> Result<i32> {
    let exit_code = run_convert(unit_file, output_dir, dry_run, force)?;

    if dry_run {
        if enable {
            eprintln!("{} would run: dinitctl enable <service>", "dry-run:".cyan().bold());
        }
        if start {
            eprintln!("{} would run: dinitctl start <service>", "dry-run:".cyan().bold());
        }
        return Ok(exit_code);
    }

    if exit_code == 2 {
        return Ok(2);
    }

    let service_name = unit_file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    if enable {
        let status = process::Command::new("dinitctl")
            .args(["enable", service_name])
            .status()
            .context("failed to run dinitctl enable")?;
        if !status.success() {
            eprintln!("{} dinitctl enable {} failed", "error:".red().bold(), service_name);
            return Ok(2);
        }
        eprintln!("{} enabled {}", "ok:".green().bold(), service_name);
    }

    if start {
        let status = process::Command::new("dinitctl")
            .args(["start", service_name])
            .status()
            .context("failed to run dinitctl start")?;
        if !status.success() {
            eprintln!("{} dinitctl start {} failed", "error:".red().bold(), service_name);
            return Ok(2);
        }
        eprintln!("{} started {}", "ok:".green().bold(), service_name);
    }

    Ok(exit_code)
}

fn run_hook() -> Result<i32> {
    let config = Config::load().context("failed to load config")?;
    sd2dinit::hook::run_hook(&config)?;
    Ok(0)
}

fn write_or_print(path: &Path, content: &str, dry_run: bool, force: bool, label: &str) -> Result<()> {
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

    fs::write(path, content)
        .with_context(|| format!("failed to write {}", path.display()))?;

    // Make shell scripts executable on Unix
    #[cfg(unix)]
    if path.extension().map(|e| e == "sh").unwrap_or(false) {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .with_context(|| format!("failed to chmod {}", path.display()))?;
    }

    Ok(())
}
