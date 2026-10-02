// SPDX-FileCopyrightText: Hadi Cherkaoui <contact@hide.cherkaoui.ch>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sd2dinit-cli-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn user_units_resolve_against_user_service_dirs_only() {
    let root = scratch("user-scope");
    let units = root.join("usr/lib/systemd/user");
    fs::create_dir_all(&units).unwrap();
    let unit = units.join("userd.service");
    fs::write(
        &unit,
        "[Unit]\nWants=pipewire.service sysd.service\n[Service]\nExecStart=/usr/bin/userd\n",
    )
    .unwrap();

    let (system_out, user_shared, xdg) = (
        root.join("system"),
        root.join("user-shared"),
        root.join("xdg"),
    );
    for dir in [
        &system_out,
        &user_shared,
        &xdg.join("dinit.d"),
        &xdg.join("sd2dinit"),
    ] {
        fs::create_dir_all(dir).unwrap();
    }
    fs::write(
        system_out.join("sysd"),
        "type = process\ncommand = /usr/bin/sysd\n",
    )
    .unwrap();
    fs::write(
        xdg.join("dinit.d/pipewire"),
        "type = process\ncommand = /usr/bin/pipewire\n",
    )
    .unwrap();
    fs::write(
        xdg.join("sd2dinit/config.toml"),
        format!(
            "output_dir = {:?}\nuser_output_dir = {:?}\nservice_dirs = []\nuser_service_dirs = [{:?}]\n",
            system_out, root.join("user-out"), user_shared
        ),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_sd2dinit"))
        .args(["convert", "--dry-run"])
        .arg(&unit)
        .env("XDG_CONFIG_HOME", &xdg)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(stdout.contains("waits-for = pipewire\n"), "{stdout}");
    assert!(!stdout.contains("sysd"), "{stdout}");
    fs::remove_dir_all(&root).unwrap();
}
