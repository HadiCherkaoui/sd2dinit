// SPDX-FileCopyrightText: Hadi Cherkaoui <contact@hide.cherkaoui.ch>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use sd2dinit::config::Config;
use sd2dinit::hook::process_targets;
use std::fs;
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sd2dinit-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn units_in_one_transaction_can_depend_on_each_other() {
    let root = scratch("batch");
    let units = root.join("usr/lib/systemd/system");
    fs::create_dir_all(&units).unwrap();
    fs::write(
        units.join("app.service"),
        "[Unit]\nWants=helper.service\nAfter=helper.service\n[Service]\nExecStart=/usr/bin/app\n",
    )
    .unwrap();
    fs::write(
        units.join("helper.service"),
        "[Service]\nExecStart=/usr/bin/helper\n",
    )
    .unwrap();

    let out = root.join("dinit.d");
    let config = Config {
        output_dir: out.clone(),
        user_output_dir: out.join("user"),
        service_dirs: Vec::new(),
        user_service_dirs: Vec::new(),
        ..Config::default()
    };

    // app comes first, so helper's dinit service does not exist yet when app converts
    let lines = ["app", "helper"].map(|n| units.join(format!("{n}.service")).display().to_string());
    process_targets(&lines, &config);

    let app = fs::read_to_string(out.join("app")).unwrap();
    assert!(app.contains("waits-for = helper\n"), "{app}");
    assert!(out.join("helper").exists());
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_sibling_that_fails_to_convert_is_not_depended_on() {
    let root = scratch("broken-sibling");
    let units = root.join("usr/lib/systemd/system");
    fs::create_dir_all(&units).unwrap();
    fs::write(
        units.join("app.service"),
        "[Unit]\nRequires=broken.service\n[Service]\nExecStart=/usr/bin/app\n",
    )
    .unwrap();
    fs::write(units.join("broken.service"), "[Service]\nType=simple\n").unwrap();

    let out = root.join("dinit.d");
    let config = Config {
        output_dir: out.clone(),
        user_output_dir: out.join("user"),
        service_dirs: Vec::new(),
        user_service_dirs: Vec::new(),
        ..Config::default()
    };
    let lines = ["app", "broken"].map(|n| units.join(format!("{n}.service")).display().to_string());
    process_targets(&lines, &config);

    let app = fs::read_to_string(out.join("app")).unwrap();
    assert!(!app.contains("depends-on"), "{app}");
    assert!(!out.join("broken").exists());
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn user_and_system_units_resolve_against_their_own_scope() {
    let root = scratch("scopes");
    let sys_units = root.join("usr/lib/systemd/system");
    let user_units = root.join("usr/lib/systemd/user");
    fs::create_dir_all(&sys_units).unwrap();
    fs::create_dir_all(&user_units).unwrap();
    fs::write(
        sys_units.join("sysd.service"),
        "[Service]\nExecStart=/usr/bin/sysd\n",
    )
    .unwrap();
    fs::write(
        user_units.join("userd.service"),
        "[Unit]\nWants=sysd.service pipewire.service\n[Service]\nExecStart=/usr/bin/userd\n",
    )
    .unwrap();
    let user_dir = root.join("user-services");
    fs::create_dir_all(&user_dir).unwrap();
    fs::write(
        user_dir.join("pipewire"),
        "type = process\ncommand = /usr/bin/pipewire\n",
    )
    .unwrap();

    let out = root.join("dinit.d");
    let config = Config {
        output_dir: out.clone(),
        user_output_dir: out.join("user"),
        service_dirs: Vec::new(),
        user_service_dirs: vec![user_dir],
        ..Config::default()
    };
    let lines = [
        sys_units.join("sysd.service"),
        user_units.join("userd.service"),
    ]
    .map(|p| p.display().to_string());
    process_targets(&lines, &config);

    let userd = fs::read_to_string(out.join("user/userd")).unwrap();
    assert!(userd.contains("waits-for = pipewire\n"), "{userd}");
    assert!(!userd.contains("sysd"), "{userd}");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn user_units_point_at_files_in_the_user_output_dir() {
    let root = scratch("user-paths");
    let units = root.join("usr/lib/systemd/user");
    fs::create_dir_all(&units).unwrap();
    fs::write(
        units.join("userd.service"),
        "[Service]\nEnvironment=A=1\nExecStartPre=/usr/bin/prep\nExecStartPre=-/usr/bin/tidy\nExecStart=/usr/bin/userd\n",
    )
    .unwrap();

    let out = root.join("dinit.d");
    let config = Config {
        output_dir: out.clone(),
        user_output_dir: out.join("user"),
        service_dirs: Vec::new(),
        user_service_dirs: Vec::new(),
        ..Config::default()
    };
    process_targets(
        &[units.join("userd.service").display().to_string()],
        &config,
    );

    let user_out = out.join("user");
    let userd = fs::read_to_string(user_out.join("userd")).unwrap();
    let pre = fs::read_to_string(user_out.join("userd-pre")).unwrap();
    assert!(
        userd.contains(&format!(
            "env-file = {}\n",
            user_out.join("userd.env").display()
        )),
        "{userd}"
    );
    assert!(user_out.join("userd.env").exists());
    assert!(pre.contains(&user_out.display().to_string()), "{pre}");
    fs::remove_dir_all(&root).unwrap();
}
