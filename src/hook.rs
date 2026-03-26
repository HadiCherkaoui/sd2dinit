use std::io::{self, BufRead};
use std::path::PathBuf;

use crate::config::Config;
use crate::converter;
use crate::generator;
use crate::model::Severity;
use crate::parser::SystemdUnit;

pub fn run_hook(config: &Config) -> anyhow::Result<()> {
    let stdin = io::stdin();
    let mut converted = 0u32;
    let mut skipped = 0u32;

    for line in stdin.lock().lines() {
        let line = line?;
        let path = PathBuf::from(line.trim());

        // Only process .service files
        if path.extension().map(|e| e != "service").unwrap_or(true) {
            continue;
        }

        // Skip template units
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            if stem.contains('@') {
                eprintln!("  skip: template unit {}", path.display());
                skipped += 1;
                continue;
            }
        }

        // Check ignored list
        if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
            if config.ignored_units.contains(&name.to_string()) {
                skipped += 1;
                continue;
            }
        }

        // Paths from the pacman hook are relative to filesystem root (e.g. "usr/lib/...")
        let full_path = if path.is_absolute() {
            path.clone()
        } else {
            PathBuf::from("/").join(&path)
        };

        if !full_path.exists() {
            continue;
        }

        match convert_unit(&full_path, config) {
            Ok(name) => {
                eprintln!("  converted: {}", name);
                converted += 1;
            }
            Err(e) => {
                eprintln!("  error converting {}: {:#}", path.display(), e);
                skipped += 1;
            }
        }
    }

    eprintln!("sd2dinit hook: {} converted, {} skipped", converted, skipped);
    Ok(())
}

fn convert_unit(path: &PathBuf, config: &Config) -> anyhow::Result<String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {}", path.display(), e))?;

    let unit = SystemdUnit::parse(&content, path.clone())
        .map_err(|e| anyhow::anyhow!("parse error: {}", e))?;

    let result = converter::convert(&unit, config)
        .map_err(|e| anyhow::anyhow!("convert error: {}", e))?;

    // Print warnings
    for w in &result.warnings {
        let prefix = match w.severity {
            Severity::Info => "info",
            Severity::Warn => "warn",
            Severity::Error => "error",
        };
        eprintln!("    {}: [{}] {}", prefix, w.directive, w.message);
    }

    // Write main service
    write_service_file(config, &result.main_service.name, &generator::generate(&result.main_service))?;

    // Write pre/post services
    if let Some(ref pre) = result.pre_service {
        write_service_file(config, &pre.name, &generator::generate(pre))?;
    }
    if let Some(ref post) = result.post_service {
        write_service_file(config, &post.name, &generator::generate(post))?;
    }

    // Write scripts
    if let Some(ref script) = result.pre_script {
        write_script_file(config, &format!("{}-pre.sh", result.main_service.name), script)?;
    }
    if let Some(ref script) = result.post_script {
        write_script_file(config, &format!("{}-post.sh", result.main_service.name), script)?;
    }
    if let Some(ref script) = result.stop_script {
        write_script_file(config, &format!("{}-stop.sh", result.main_service.name), script)?;
    }

    // Write env file
    if let Some(ref env_content) = result.env_file_content {
        let path = config.output_dir.join(format!("{}.env", result.main_service.name));
        std::fs::write(&path, env_content)
            .map_err(|e| anyhow::anyhow!("failed to write {}: {}", path.display(), e))?;
    }

    Ok(result.main_service.name.clone())
}

fn write_service_file(config: &Config, name: &str, content: &str) -> anyhow::Result<()> {
    let path = config.output_dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow::anyhow!("failed to create dir {}: {}", parent.display(), e))?;
    }
    std::fs::write(&path, content)
        .map_err(|e| anyhow::anyhow!("failed to write {}: {}", path.display(), e))
}

fn write_script_file(config: &Config, name: &str, content: &str) -> anyhow::Result<()> {
    let path = config.output_dir.join(name);
    std::fs::write(&path, content)
        .map_err(|e| anyhow::anyhow!("failed to write {}: {}", path.display(), e))?;

    // Make executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| anyhow::anyhow!("failed to chmod {}: {}", path.display(), e))?;
    }

    Ok(())
}
