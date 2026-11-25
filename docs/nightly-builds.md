# AniCraft Nightly builds

The Nightly flavor is the preview channel the ArtCraft Launcher installs beside stable AniCraft:
one app, two channels, no shared state. Everything here is switched off in CI (no schedule, no
release branch, no signing); this page describes what exists and how to exercise it.

## The contract

| Item | Value |
|---|---|
| Build switch | `ANICRAFT_BUILD_CHANNEL=nightly-v1` makes `build.rs` emit `cfg(anicraft_nightly_v1)`. Unset, empty or `stable` is stable; anything else is a build error. |
| Version | `ANICRAFT_VERSION=X.Y.Z-nightly.YYYYMMDD.<run>.<attempt>.g<sha12>`, with `ANICRAFT_BUILD_SHA` (40 hex) and `ANICRAFT_BUILD_DATE` (`YYYY-MM-DD`) that must match it. A nightly build without them fails in `build.rs`. |
| Version query | `--version`, `-V`, and the Launcher's `--launch-profile nightly-v1 --profile-root <abs> --version` print exactly `AniCraft <version> (nightly-v1)` on stdout, nothing on stderr, exit 0 — before logging, the profile check, IPC or any window. `AniCraft Nightly` is the display name (window title, Windows ProductName, desktop `Name=`), never part of this line. |
| Launch | `--launch-profile nightly-v1 --profile-root <abs> -- [files…]`, as separate tokens. A nightly build without a profile is refused in a native dialog ("Open AniCraft Nightly from ArtCraft Launcher"); a stable build refuses the profile. |
| Identities | Package, macOS bundle and Launcher: `ai.storyteller.anicraft.nightly`. Window/Wayland app id and Windows AUMID: `ai.storyteller.anicraft.nightly-v1`. Two different strings; `src/platform.rs` keeps both. |
| Assets | `anicraft-<v>-windows-x86_64-nightly.unsigned.zip` (the compiler's intermediate), `anicraft-<v>-linux-x86_64-nightly.tar.gz` (`anicraft` the launcher, `anicraft-bin` the GUI, the icon and the notices), `anicraft-<v>-macos-aarch64-nightly.zip` holding `AniCraft Nightly.app`. GUI only: no `anicraft-cli`, portable marker, `.desktop`, DMG, associations or URL types. |

## Data

Nightly writes **everything** into the profile root the Launcher names, and nowhere else:

| State | Path inside the profile | Source |
|---|---|---|
| Preferences | `prefs.json` | `platform::config_dir` |
| Recovery and autosave | `Recovery/` | `platform::recovery_dir` |
| Start-up log | `Logs/` | `platform::log_dir` |
| Plug-ins | `plugins/` | `platform::plugins_dir` |
| Templates, saved commands, themes | `Templates/`, `Commands/`, `themes/` | `platform::templates_dir`, `commands_dir`, `themes_dir` |
| Start-screen thumbnails | `Thumbnails/` | `platform::save_thumbnail` |
| User guide (cache) | `Cache/guide-<version>/` | `platform::guide_dir` |
| Nightly session health | `nightly-session.json`, `nightly-session.lock` | `src/nightly_session.rs` |

`platform::placement()` is the one switch: a validated profile replaces the portable and installed
layouts wholesale, so every folder above follows it and stable's is never read. The profile is
validated before the first folder is created (`src/profile_root.rs`): it must not overlap stable's
`%APPDATA%\AniCraft`, `%LOCALAPPDATA%\AniCraft`, `Library/{Application Support,Caches,Preferences}/
AniCraft`, `$XDG_{CONFIG,DATA,CACHE,STATE}_HOME/anicraft`, the Flatpak folder
`~/.var/app/ai.storyteller.anicraft`, `/usr/bin/anicraft`, `/usr/local/bin/anicraft`,
`/opt/anicraft`, or the installation (the `.app` on macOS); it must not be inside a portable copy
(`anicraft-portable`) or one of its `data` folders; every entry in the profile must resolve inside
it (no symlinked `Recovery`); and it is refused when `ANICRAFT_RECOVERY_DIR`,
`ANICRAFT_TEMPLATES_DIR`, `ANICRAFT_COMMANDS_DIR`, `ANICRAFT_THEMES_DIR`, `ANICRAFT_LOG_DIR` or
`ANICRAFT_PLUGINS` is inherited, or when a native build inherits another app's `FLATPAK_ID`
(the Nightly Flatpak itself is allowed, and its profile must stay in its own home).

No single-instance socket exists in AniCraft (a second launch is a second window); the control
port's `control.json` is not written either, because Nightly never opens the control port.

## Building

```sh
# Windows (PowerShell; the unsigned intermediate the signer consumes)
$env:ANICRAFT_BUILD_CHANNEL='nightly-v1'; $env:ANICRAFT_VERSION='0.3.0-nightly.20261124.17.1.gabcdef012345'
$env:ANICRAFT_BUILD_SHA='abcdef0123456789abcdef0123456789abcdef01'; $env:ANICRAFT_BUILD_DATE='2026-11-24'
pwsh packaging/windows/package.ps1 -Arch x64 -Nightly -BuildOnly

# Linux
bash packaging/linux/nightly.sh --arch x86_64

# macOS (Apple silicon)
bash packaging/macos/nightly.sh --arch aarch64
bash packaging/macos/nightly-verify.sh --expect unsigned --input dist/anicraft-<v>-macos-aarch64-nightly.zip
```

All three read the same four `ANICRAFT_*` values plus an absolute `CARGO_TARGET_DIR` and `DIST`
(`packaging/nightly-common.sh`), build only the GUI crate `anicraft` with its default features,
refuse a dirty tree, and never accept signing material. `packaging/nightly-tests.py` runs the
synthetic suites (archive policy, the launcher in all five layouts, plist rendering, the Apple
scripts against stand-in tools); a
three-platform hosted check runs the real scripts with the `full-ci` label
(`.github/workflows/nightly-builds.yml`).

Signing is the trusted step's: `packaging/windows/sign.ps1 -Required`,
`packaging/macos/nightly-sign.sh`, and the Linux key `.github/nightly-linux-signing-key.asc`
(byte-identical to `release-signing-key.asc` in the stable release assets; fingerprint
`2B76741B414AAD6CD33E631213A005C0B9D7C3ED`).

## Distribution wrappers

Every Linux nightly format starts the program the flavor's way, through the one launcher
`packaging/linux/nightly-launch.sh`:

```sh
exec <real-program> --launch-profile nightly-v1 --profile-root "${XDG_DATA_HOME:-$HOME/.local/share}/anicraft-nightly/profile" -- "$@"
```

| Format | Installs the launcher as | Real program |
|---|---|---|
| `.deb` / `.rpm` | `/usr/bin/anicraft-nightly` | `/usr/lib/anicraft-nightly/anicraft` |
| AppImage | `$APPDIR/AppRun` (a script, not a symlink) | `$APPDIR/usr/bin/anicraft` |
| Arch handoff | `bin/anicraft` (shipped) | `bin/.anicraft-nightly/anicraft` |
| Flatpak | `/app/bin/anicraft-nightly` (the bundle's `command:`) | `/app/bin/anicraft-bin` |
| nightly `.tar.gz` | `anicraft` (the standard layout: the launcher keeps the app's name) | `anicraft-bin` beside it |

The profile is a sibling of stable's data folder, never inside it and never containing it, and
`anicraft --version` from the tar (or `anicraft-nightly --version` from a package) answers the
Launcher's own query form. A command line that already carries `--launch-profile` goes straight to
the program instead of being wrapped twice, and only the deb/rpm layout looks in
`/usr/lib/anicraft-nightly`; a bundle never trusts a folder beside its mount. A package whose
launcher cannot put arguments in front of the person's own may name the root in
`ANICRAFT_PROFILE_ROOT` instead; nothing here uses it, only a nightly build reads it, it is
validated exactly like the Launcher's root, and it may not be combined with
`--launch-profile`/`--profile-root`.

## Callers and policy

The enabled callers `nightly.yml` (producer) and `nightly-sign.yml` (release-ref signer), the
installed policy `.github/nightly-signing-policy.json` and their contract tests
(`packaging/nightly-release-tests.py`) are dylanmtaylor's, kept exactly as he wrote them; this
change adds only the Linux public key they require. They are inert until the shared generalization
lands in `craft-common-actions`, so two properties hold instead of dormancy: neither caller has a
`schedule:` trigger (a run is an explicit manual dispatch), and the manual actors are the five
release managers (`bflatastic`, `dylanmtaylor`, `echelon`, `FredApps`, `skylartaylor`).
`nightly.yml` is registered only once it lands on `main`, which is when the policy's
`producer_workflow_id` can be filled in.
