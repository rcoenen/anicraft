# Packaging

How AniCraft is built into installers (ADR 0026). The family's shared release workflow
(`storytold/craft-common-actions`) runs these scripts; `docs/release.md` describes a release.

| Path | What |
|---|---|
| `env.sh` | Version, build variables and `APP_ID`, sourced by every script |
| `check-icons.sh` | The icons every package names exist |
| `check-store-readiness.sh` | Store metadata checks: Flatpak manifests, AppStream, desktop entry, winget/MSIX (ADR 0094) |
| `windows/` | MSI (WiX v5), portable zip, symbols zip; install policy checks ([README](windows/README.md)) |
| `macos/` | Universal `AniCraft.app` on a DMG, CLI zip, signing and notarization |
| `linux/` | AppImage, deb, rpm, tar.gz; Flatpak manifests; desktop entry, metainfo, MIME types |
| `web/` | The web build's zip |

## Portable copies (ADR 0094)

The Windows zip and the Linux tar.gz contain a file named `anicraft-portable` at their top. A
copy with that marker keeps preferences, autosave, logs, plug-ins, templates, thumbnails and the
offline guide in a `data/` folder beside the marker instead of the user's settings folder. The
app looks for the marker next to the program, above `bin/`, next to `AniCraft.app` (macOS) and
next to the `.AppImage`. Installed packages (MSI, DMG, deb, rpm, Flatpak, Arch) never ship it.
Preferences › About shows which kind of copy is running and its folders.

## AppImage desktop integration (ADR 0094)

The AppImage's `AppRun` starts the app directly. On the first start, the app offers once
(non-modally) to add itself to the applications menu; the engine's `desktop.integrate` then writes
the desktop entry (with `Exec` pointing at the `.AppImage`), the MIME types and the hicolor icons
from this folder into `$XDG_DATA_HOME` (`~/.local/share`), never into system folders.
Preferences › About removes them (`desktop.remove`). Headless:

```sh
anicraft-cli exec --sample desktop.integrate '{"appimage": "/home/me/Apps/AniCraft.AppImage", "data_home": "/home/me/.local/share"}'
```

## Store readiness

`check-store-readiness.sh` runs in the Packaging lint workflow (`store-readiness` job) and
locally (it skips tools that aren't installed). It validates; it never submits. Submissions to
Flathub, winget and the Microsoft Store are made by the team. winget manifests go in
`windows/winget/*.yaml` and an MSIX manifest in `windows/msix/AppxManifest.xml`; the check reads
their required fields once they exist.

## Nightly

The preview channel the ArtCraft Launcher installs beside stable AniCraft has its own scripts,
which the stable ones never source: `nightly-common.sh` (the four `ANICRAFT_*` values and the
frozen, clean tree), `linux/nightly.sh` with `linux/nightly-launch.sh` (the one launcher every
Linux nightly format installs; the plain tar is the standard layout, `anicraft` the launcher and
`anicraft-bin` the GUI it starts), `macos/nightly.{sh,sh-sign,sh-verify}` with
`macos/nightly-lib.sh` and `macos/nightly_plist.py`, and `windows/nightly.ps1` behind
`windows/package.ps1 -Nightly -BuildOnly`. Every artifact is GUI only: no `anicraft-cli`, portable
marker, `.desktop`, DMG, association or URL type. Signing is the trusted step's
(`windows/sign.ps1 -Required`, `macos/nightly-sign.sh`, the Linux key in `.github/`).

`nightly-tests.py` runs the synthetic suites (archive policy, the launcher in each format's
layout, icon/plist rendering, and the Apple scripts against stand-in tools), and `windows/nightly-{package,signing}-tests.ps1` check the
spec, the source identity, the flavor binding and the compiler/signer boundary. The three
platforms are built for real, unsigned, by `.github/workflows/nightly-builds.yml` behind the
`full-ci` label. See `docs/nightly-builds.md` for the contract and the data root.
