// SPDX-FileCopyrightText: Hadi Cherkaoui <contact@hide.cherkaoui.ch>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A scratch root with a private `home/.config` and a shared user service dir.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("sd2dinit-cli-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let scratch = Self { root };
        for dir in [
            "system",
            "user-shared",
            "units/usr/lib/systemd/user",
            "home/.config/dinit.d",
            "home/.config/sd2dinit",
        ] {
            fs::create_dir_all(scratch.root.join(dir)).unwrap();
        }
        fs::write(
            scratch.root.join("system/sysd"),
            "type = process\ncommand = /usr/bin/sysd\n",
        )
        .unwrap();
        fs::write(
            scratch.root.join("user-shared/wireplumber"),
            "type = process\ncommand = /usr/bin/wireplumber\n",
        )
        .unwrap();
        fs::write(
            scratch.private().join("pipewire"),
            "type = process\ncommand = /usr/bin/pipewire\n",
        )
        .unwrap();
        fs::write(
            scratch.root.join("home/.config/sd2dinit/config.toml"),
            format!(
                "output_dir = {:?}\nuser_output_dir = {:?}\nservice_dirs = []\nuser_service_dirs = [{:?}]\n",
                scratch.root.join("system"),
                scratch.root.join("user-out"),
                scratch.root.join("user-shared"),
            ),
        )
        .unwrap();
        scratch
    }

    fn private(&self) -> PathBuf {
        self.root.join("home/.config/dinit.d")
    }

    fn unit(&self, dir: &str, name: &str) -> PathBuf {
        let unit = self.root.join(dir).join(format!("{name}.service"));
        fs::create_dir_all(unit.parent().unwrap()).unwrap();
        fs::write(
            &unit,
            "[Unit]\nWants=pipewire.service wireplumber.service sysd.service\n[Service]\nExecStart=/usr/bin/userd\n",
        )
        .unwrap();
        unit
    }

    fn sd2dinit(&self, unit: &Path, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_sd2dinit"))
            .arg("convert")
            .args(args)
            .arg(unit)
            .env("HOME", self.root.join("home"))
            .env_remove("XDG_CONFIG_HOME")
            .output()
            .unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn shared_user_output_does_not_depend_on_private_services() {
    let scratch = Scratch::new("shared");
    let unit = scratch.unit("units/usr/lib/systemd/user", "userd");
    let stdout = String::from_utf8(scratch.sd2dinit(&unit, &["--dry-run"]).stdout).unwrap();

    assert!(stdout.contains("waits-for = wireplumber\n"), "{stdout}");
    assert!(!stdout.contains("pipewire"), "{stdout}");
    assert!(!stdout.contains("sysd"), "{stdout}");
}

#[test]
fn own_output_dir_may_depend_on_private_services() {
    let scratch = Scratch::new("own");
    let unit = scratch.unit("units/usr/lib/systemd/user", "userd");
    let private = scratch.private().display().to_string();
    let stdout = String::from_utf8(
        scratch
            .sd2dinit(&unit, &["--dry-run", "--output-dir", &private])
            .stdout,
    )
    .unwrap();

    assert!(stdout.contains("waits-for = pipewire\n"), "{stdout}");
    assert!(stdout.contains("waits-for = wireplumber\n"), "{stdout}");
}

#[test]
fn own_user_units_default_to_own_dinit_dir() {
    let scratch = Scratch::new("own-unit");
    let unit = scratch.unit("home/.config/systemd/user", "mine");
    let output = scratch.sd2dinit(&unit, &[]);

    let written = fs::read_to_string(scratch.private().join("mine")).unwrap_or_else(|e| {
        panic!("{e}: {}", String::from_utf8_lossy(&output.stderr));
    });
    assert!(written.contains("waits-for = pipewire\n"), "{written}");
}
