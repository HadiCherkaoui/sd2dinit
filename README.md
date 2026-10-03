<!--
SPDX-FileCopyrightText: Hadi Cherkaoui <contact@hide.cherkaoui.ch>

SPDX-License-Identifier: AGPL-3.0-or-later
-->

# sd2dinit

**sd2dinit** converts systemd `.service` unit files into [dinit](https://davmac.org/projects/dinit/) service files. It runs as a standalone CLI or as a pacman/alpm hook that automatically converts units whenever packages are installed or upgraded.

Built for [Artix Linux](https://artixlinux.org/) and any other dinit-based distribution.

---

## Features

- Converts systemd `.service` files to dinit service format
- Maps service types: `simple → process`, `forking → bgprocess`, `oneshot → scripted`
- Resolves dependencies: `Requires→depends-on`, `Wants→waits-for`, `After→after`, `Before→before`, only onto dinit services that exist
- Handles `ExecStartPre`/`ExecStartPost` as separate dinit services with wrapper scripts
- Generates `.env` files from inline `Environment=` directives
- Merges drop-in overrides (`.d/` directories) automatically, in the CLI and the hook
- Warns about unsupported directives (sandboxing, cgroup, socket activation) without failing
- `--dry-run` mode to preview output without writing any files
- Pacman hook for automatic conversion on package install/upgrade

---

## Installation

### One-liner (recommended)

Installs to `/usr/local/bin/sd2dinit` using `doas` (or `sudo` as fallback):

```sh
curl -fsSL https://gitlab.cherkaoui.ch/HadiCherkaoui/sd2dinit/-/raw/main/install.sh | sh
```

Also installs the pacman hook automatically. To skip it:

```sh
curl -fsSL https://gitlab.cherkaoui.ch/HadiCherkaoui/sd2dinit/-/raw/main/install.sh | sh -s -- --no-hook
```

Pin a specific version:

```sh
curl -fsSL https://gitlab.cherkaoui.ch/HadiCherkaoui/sd2dinit/-/raw/main/install.sh | sh -s -- --version v0.2.0
```

### Via cargo

```sh
cargo install sd2dinit
```

Note: `cargo install` puts the binary in `~/.cargo/bin/`. If you want the pacman hook to work, copy it to a system path first:

```sh
doas install -Dm755 ~/.cargo/bin/sd2dinit /usr/local/bin/sd2dinit
doas install -Dm644 hooks/sd2dinit.hook /usr/share/libalpm/hooks/sd2dinit.hook
```

### Manual (from releases)

1. Download the latest binary from the [Releases page](https://gitlab.cherkaoui.ch/HadiCherkaoui/sd2dinit/-/releases)
2. Install it:
   ```sh
   doas install -Dm755 sd2dinit-linux-x86_64 /usr/local/bin/sd2dinit
   ```
3. Optionally install the pacman hook:
   ```sh
   doas install -Dm644 hooks/sd2dinit.hook /usr/share/libalpm/hooks/sd2dinit.hook
   ```

### Build from source

Requires a Rust toolchain that supports edition 2024.

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
| several `ExecStart=` (`Type=oneshot`) | `nginx-start.sh` (runs them in order) |
| `ExecStartPre=` | `nginx-pre` (scripted service the main one depends on) |
| `ExecStartPost=` | `nginx-post` (scripted service, see below) |
| `Environment=` | `nginx.env` (env file) |
| several `ExecStop=`, or `ExecStop=` + `ExecStopPost=` | `nginx-stop.sh` (wrapper script) |

`nginx-pre` gets the main service's environment and dependencies, so it runs
after them as `ExecStartPre=` does under systemd. dinit has no post-start hook:
`nginx-post` waits for `nginx` but only runs when it is enabled itself.
`sd2dinit install --enable` enables both; after a hook conversion, run
`dinitctl enable nginx-post` too.

Exit codes: `0` = success, `1` = success with warnings, `2` = failure.

---

## Configuration

Create `~/.config/sd2dinit/config.toml` (or `$XDG_CONFIG_HOME/sd2dinit/config.toml`).
If it does not exist, `/etc/sd2dinit/config.toml` is read instead, which is
where the pacman hook running as root picks up a system-wide config:

```toml
# Where to write generated dinit service files (default: /etc/dinit.d)
output_dir = "/etc/dinit.d"

# Where units from usr/lib/systemd/user go (default: /usr/lib/dinit.d/user)
user_output_dir = "/usr/lib/dinit.d/user"

# Unit filenames to never convert
ignored_units = [
    "systemd-tmpfiles-setup.service",
    "systemd-journal-flush.service",
]

# Where dinit services that dependencies may point at are looked up
# (defaults: dinit's own search path, see dinit(8))
service_dirs = ["/etc/dinit.d", "/run/dinit.d", "/usr/local/lib/dinit.d", "/lib/dinit.d"]
user_service_dirs = ["/etc/dinit.d/user", "/usr/lib/dinit.d/user", "/usr/local/lib/dinit.d/user"]

# Custom systemd dependency name → dinit service name mappings
[dependency_map]
"display-manager.service" = "sddm"
```

### How dependencies are resolved

dinit refuses to load a service whose `depends-on` or `waits-for` names a
service it cannot find, so each systemd dependency is matched against the
services that actually exist in `service_dirs` (or `user_service_dirs` for user
units) and the output directory, plus the units that convert in the same pacman
transaction.

`sd2dinit convert` writes a unit from `~/.config/systemd/user` to your own
`~/.config/dinit.d`, and any other user unit to `user_output_dir`; a unit from
another user's home needs an explicit `--output-dir`. Only when it
writes into your own directory does it also resolve against `$XDG_CONFIG_HOME/dinit.d`
and `~/.config/dinit.d`; a service in the shared directory must not depend on
something only you have. Each candidate is matched in this order:

1. an entry in `dependency_map`, used as-is even if no such service exists yet;
2. the exact name, e.g. `network.target` (Artix ships it);
3. the name without its unit suffix, e.g. `dbus.service` → `dbus`.

If none matches, the dependency is dropped with a warning. A dependency that
resolves to the unit itself (`docker.service` → `Requires=docker.socket`) is
dropped too.

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
| `simple` (default), `exec`, `idle` | `process` | |
| `forking` + `PIDFile=` | `bgprocess` | |
| `forking` (no PIDFile) | `process` | Warning emitted, falls back |
| `oneshot` | `scripted` | |
| `dbus` | `process` | Warning: dbus activation not supported |
| `notify` | `process` | Warning: notify not supported |

### Dependencies

| systemd | dinit |
|---|---|
| `Requires=` | `depends-on` |
| `Wants=` | `waits-for` (starts it, but its failure does not block this service) |
| `After=` | `after` (ordering only, never starts it) |
| `Before=` | `before` |
| `Conflicts=` | skipped (no equivalent) |

A service named under several directives keeps only the strongest relation
(`depends-on` > `waits-for` > `after`). `Before=` beats all of them: dinit
rejects `before` plus a pull-in of the same service as a cycle, so the pull-in is
dropped with a warning.

### User and Group

dinit's `run-as` takes a user only and always uses that user's primary group,
so `User=` becomes `run-as`. A `Group=` that differs from the user, or a
`Group=` without `User=`, is reported and dropped.

A numeric `User=` is kept but warned about: given a UID, dinit keeps its own
group and drops supplementary groups. Use the user name instead.

User services belong to each user's dinit instance: `sd2dinit install --enable`
calls `dinitctl --user` for them, so run it as that user, not through `doas`.

### Restart

| systemd `Restart=` | dinit `restart` |
|---|---|
| `no` (default) | `false` (written out: dinit's own default is to restart) |
| `always` | `true` |
| `on-success` | `true` (lossy — warning emitted) |
| `on-failure` / `on-abnormal` / `on-abort` | `on-failure` |

`RestartSec=` accepts systemd time spans (`3`, `500ms`, `1min 30s`, `2d`, from
`ns` up to `y`) and becomes `restart-delay` in seconds.

### Command lines

systemd runs `Exec*=` lines without a shell, after unquoting them itself, so
sd2dinit splits each line into the same argv systemd would and quotes every word
again for where it ends up: dinit only understands double quotes, and a
generated `/bin/sh` script would otherwise expand `*`, `~` or `;`.

- `'…'`, `"…"` and C escapes such as `\s` and `\x41` are unquoted as systemd
  does, and a lone `;` separates several commands on one line.
- `$VAR` standing alone splits into words (dinit's `$/VAR`), `${VAR}` is always
  one word, `$$` is a literal `$`, and a bare `$VAR` or `${VAR:-default}` inside
  a word stays literal, all as in systemd. dinit splits `$/VAR` at whitespace
  only, so quotes inside such a value are reported.
- `%n`, `%N`, `%p` and `%%` are expanded; other specifiers are dropped with a warning.
- dinit never sets `$MAINPID`. A lone `ExecStop=kill $MAINPID` is dropped, since
  dinit signals the process itself when no stop command is set: `-INT`, `-QUIT`
  and `-KILL` become `term-signal`, and signals that do not stop the process are
  reported. Any other use of `$MAINPID` is warned about.
- `KillSignal=` becomes `term-signal`.
- `ExecStopPost=` runs even when `ExecStop=` fails, and the stop script then
  exits with `ExecStop=`'s status.

### Exec prefixes

systemd's special prefixes on `ExecStart=`, `ExecStop=`, `ExecStartPre=`,
`ExecStartPost=` and `ExecStopPost=` are stripped from the command:

| Prefix | Handling |
|---|---|
| `-` | lines in generated scripts get `\|\| true`, including a oneshot's `ExecStart=`; on any other `ExecStart=`, or a lone `ExecStop=`, it is reported and dropped |
| `:` | no variable is substituted; `$` stays literal in dinit and in scripts |
| `@` | dinit cannot set argv[0]; the argv[0] word is dropped with a warning |
| `+`, `!`, `!!`, `\|` | full privileges and running through the user's shell are not supported; warning emitted |
| prefix only, no command | the line is dropped with a warning; without a usable `ExecStart=` the unit is not converted |

### Out of scope (warnings emitted, directives skipped)

- Sandboxing: `ProtectSystem`, `PrivateTmp`, `NoNewPrivileges`, etc.
- CGroup/resource limits: `Slice`, `CPUQuota`, `MemoryMax`, `TasksMax`, etc.
- Conditionals: `ConditionPathExists`, `AssertPathExists`, etc.
- Socket activation: `ListenStream`, `ListenDatagram`, etc.
- Template/instance units: `name@.service`

---

## License

Copyright © 2026 Hadi Cherkaoui

This program is free software: you can redistribute it and/or modify it under the terms of the GNU Affero General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.

See [LICENSE](LICENSE) for the full text.
