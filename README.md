# sd2dinit

**sd2dinit** converts systemd `.service` unit files into [dinit](https://davmac.org/projects/dinit/) service files. It runs as a standalone CLI or as a pacman/alpm hook that automatically converts units whenever packages are installed or upgraded.

Built for [Artix Linux](https://artixlinux.org/) and any other dinit-based distribution.

---

## Features

- Converts systemd `.service` files to dinit service format
- Maps service types: `simple → process`, `forking → bgprocess`, `oneshot → scripted`
- Resolves dependencies: `Requires→depends-on`, `Wants→depends-ms`, `After→waits-for`
- Handles `ExecStartPre`/`ExecStartPost` as separate dinit services with wrapper scripts
- Generates `.env` files from inline `Environment=` directives
- Merges drop-in overrides (`.d/` directories) automatically
- Warns about unsupported directives (sandboxing, cgroup, socket activation) without failing
- `--dry-run` mode to preview output without writing any files
- Pacman hook for automatic conversion on package install/upgrade

---

## Installation

### One-liner (recommended)

```sh
curl -fsSL https://gitlab.cherkaoui.ch/HadiCherkaoui/sd2dinit/-/raw/main/install.sh | sh
```

This installs the binary to `~/.local/bin/` (no escalation needed). If that directory is not in your `PATH`, the installer will tell you.

To install globally to `/usr/local/bin/` instead:

```sh
curl -fsSL https://gitlab.cherkaoui.ch/HadiCherkaoui/sd2dinit/-/raw/main/install.sh | sh -s -- --global
```

To skip the pacman hook:

```sh
curl -fsSL https://gitlab.cherkaoui.ch/HadiCherkaoui/sd2dinit/-/raw/main/install.sh | sh -s -- --no-hook
```

### Manual (from releases)

1. Download the latest binary from the [Releases page](https://gitlab.cherkaoui.ch/HadiCherkaoui/sd2dinit/-/releases)
2. Make it executable and move it into your PATH:
   ```sh
   chmod +x sd2dinit-linux-x86_64
   mv sd2dinit-linux-x86_64 ~/.local/bin/sd2dinit
   ```
3. Optionally install the pacman hook:
   ```sh
   doas cp hooks/sd2dinit.hook /usr/share/libalpm/hooks/
   ```

### Build from source

Requires Rust 1.70+.

```sh
git clone https://gitlab.cherkaoui.ch/HadiCherkaoui/sd2dinit.git
cd sd2dinit
cargo build --release
sudo cp target/release/sd2dinit /usr/local/bin/
```

---

## Usage

### Convert a unit file

Preview the generated dinit service without writing anything:

```sh
sd2dinit convert /usr/lib/systemd/system/sshd.service --dry-run
```

Write to a directory:

```sh
sd2dinit convert /usr/lib/systemd/system/sshd.service --output-dir /etc/dinit.d/
```

Overwrite an existing file:

```sh
sd2dinit convert /usr/lib/systemd/system/sshd.service --force
```

### Convert and enable/start

```sh
# Convert, enable (auto-start on boot), and start now
doas sd2dinit install /usr/lib/systemd/system/nginx.service --enable --start

# Just convert and enable, don't start yet
doas sd2dinit install /usr/lib/systemd/system/nginx.service --enable
```

### Pacman hook mode

The hook runs automatically when pacman installs or upgrades packages. You can also trigger it manually:

```sh
echo "usr/lib/systemd/system/sshd.service" | sd2dinit hook
```

---

## Output

For a service like `nginx.service`:

| Input | Generated output |
|---|---|
| `ExecStart=` | `nginx` (main service file) |
| `ExecStartPre=` | `nginx-pre` (scripted service) |
| `ExecStartPost=` | `nginx-post` (scripted service) |
| `Environment=` | `nginx.env` (env file) |
| `ExecStop=` + `ExecStopPost=` | `nginx-stop.sh` (wrapper script) |

Exit codes: `0` = success, `1` = success with warnings, `2` = failure.

---

## Configuration

Create `~/.config/sd2dinit/config.toml`:

```toml
# Where to write generated dinit service files (default: /etc/dinit.d)
output_dir = "/etc/dinit.d"

# Unit filenames to never convert
ignored_units = [
    "systemd-tmpfiles-setup.service",
    "systemd-journal-flush.service",
]

# Custom systemd dependency name → dinit service name mappings
# These augment the built-in defaults (see below)
[dependency_map]
"NetworkManager.service" = "NetworkManager"
"dbus.service" = "dbus"
```

### Built-in dependency mappings

| systemd name | dinit name |
|---|---|
| `network-online.target` | `network` |
| `network.target` | `network` |
| `multi-user.target` | `boot` |
| `sysinit.target` | `boot` |
| `default.target` | `boot` |

User-defined entries in `config.toml` override these defaults.

---

## Pacman hook

After installing sd2dinit, enable the automatic conversion hook:

```sh
doas cp hooks/sd2dinit.hook /usr/share/libalpm/hooks/
```

Once installed, any `pacman -S` or `pacman -U` that includes a `.service` file will automatically trigger sd2dinit. You'll see output like:

```
:: Converting systemd units to dinit...
  converted: nginx
  converted: sshd
sd2dinit hook: 2 converted, 0 skipped
```

To skip conversion for specific units, add them to `ignored_units` in your config.

---

## Conversion reference

### Service types

| systemd `Type=` | dinit `type` | Notes |
|---|---|---|
| `simple` (default) | `process` | |
| `forking` + `PIDFile=` | `bgprocess` | |
| `forking` (no PIDFile) | `process` | Warning emitted, falls back |
| `oneshot` | `scripted` | |
| `dbus` | `process` | Warning: dbus activation not supported |
| `notify` | `process` | Warning: notify not supported |

### Dependencies

| systemd | dinit |
|---|---|
| `Requires=` | `depends-on` |
| `Wants=` | `depends-ms` |
| `After=` | `waits-for` |
| `Before=` | skipped (no equivalent) |
| `Conflicts=` | skipped (no equivalent) |

### Restart

| systemd `Restart=` | dinit `restart` |
|---|---|
| `no` | (omitted) |
| `always` | `true` |
| `on-success` | `true` (lossy — warning emitted) |
| `on-failure` / `on-abnormal` / `on-abort` | `on-failure` |

### Out of scope (warnings emitted, directives skipped)

- Sandboxing: `ProtectSystem`, `PrivateTmp`, `NoNewPrivileges`, etc.
- CGroup/resource limits: `Slice`, `CPUQuota`, `MemoryMax`, `TasksMax`, etc.
- Conditionals: `ConditionPathExists`, `AssertPathExists`, etc.
- Socket activation: `ListenStream`, `ListenDatagram`, etc.
- Template/instance units: `name@.service`

---

## License

Copyright © 2026 Hadi Cherkaoui

This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.

See [LICENSE](LICENSE) for the full text.
