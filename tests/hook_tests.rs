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
