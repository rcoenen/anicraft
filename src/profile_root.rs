//! App-owned nightly profile fences; no stable fallback, migration or process environment changes.
//!
//! Nightly keeps every piece of user state in one private folder the Launcher created for it:
//! `prefs.json`, `Recovery`, `Logs`, `plugins`, `Templates`, `Commands`, `themes`, `Thumbnails`,
//! the user guide's cache and the Nightly session record (`src/platform.rs`,
//! `src/nightly_session.rs`). This module decides whether a root the Launcher named is really a
//! private place, never an alias of the stable app's data or of the installation itself.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

/// The nightly archive names (`packaging/nightly-common.sh`, `packaging/macos/nightly.sh`).
const ARCHIVE_BINARIES: [&str; 2] = ["anicraft.exe", "anicraft"];

/// The environment variables that would move one of Nightly's folders somewhere else, and the
/// Windows/Linux/macOS install folders a profile must never sit in.
const FOLDER_ENV: [&str; 6] =
    ["ANICRAFT_RECOVERY_DIR", "ANICRAFT_TEMPLATES_DIR", "ANICRAFT_COMMANDS_DIR", "ANICRAFT_THEMES_DIR", "ANICRAFT_LOG_DIR", "ANICRAFT_PLUGINS"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Platform {
    Windows,
    Linux,
    Mac,
}

impl Platform {
    fn current() -> Result<Self, String> {
        match std::env::consts::OS {
            "windows" => Ok(Self::Windows),
            "linux" => Ok(Self::Linux),
            "macos" => Ok(Self::Mac),
            _ => Err(tr!("nightly-v1 is not defined for this operating system").into()),
        }
    }

    fn ignores_case(self) -> bool {
        self != Self::Linux
    }
}

fn absolute(path: PathBuf, label: &str) -> Result<PathBuf, String> {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(tr!("{label} must be absolute and contain no parent traversal", label = label));
    }
    Ok(path)
}

fn required_base(env: &impl Fn(&str) -> Option<OsString>, key: &str) -> Result<PathBuf, String> {
    let value = env(key).filter(|v| !v.is_empty()).ok_or_else(|| tr!("cannot establish the stable {key} boundary", key = key))?;
    absolute(PathBuf::from(value), key)
}

fn xdg_base(env: &impl Fn(&str) -> Option<OsString>, key: &str, fallback: &str) -> Result<PathBuf, String> {
    match env(key).filter(|v| !v.is_empty()) {
        Some(value) => absolute(PathBuf::from(value), key),
        None => Ok(required_base(env, "HOME")?.join(fallback)),
    }
}

/// Resolve existing ancestors before creating anything. A dangling link or inaccessible ancestor
/// is not a missing directory that may be replaced or followed later.
fn canonical_future(path: &Path) -> Result<PathBuf, String> {
    let mut parent = path;
    let mut missing = Vec::new();
    loop {
        match std::fs::symlink_metadata(parent) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(parent.file_name().ok_or_else(|| tr!("the profile root has no existing ancestor"))?.to_os_string());
                parent = parent.parent().ok_or_else(|| tr!("the profile root has no existing ancestor"))?;
            }
            Err(error) => return Err(tr!("cannot inspect the profile boundary: {e}", e = error)),
        }
    }
    let mut resolved = std::fs::canonicalize(parent).map_err(|e| tr!("cannot resolve the profile boundary: {e}", e = e))?;
    for part in missing.into_iter().rev() {
        resolved.push(part);
    }
    Ok(resolved)
}

fn overlaps(a: &Path, b: &Path, platform: Platform) -> bool {
    let parts = |p: &Path| {
        p.components()
            .map(|c| {
                let value = c.as_os_str().to_string_lossy().into_owned();
                if platform.ignores_case() { value.to_lowercase() } else { value }
            })
            .collect::<Vec<_>>()
    };
    let (a, b) = (parts(a), parts(b));
    a.starts_with(&b) || b.starts_with(&a)
}

/// The folder a profile must not sit inside: the installation (a `.app` on macOS), since writing
/// beside the executable would land in a versioned install folder.
fn installation_boundary(platform: Platform, exe: &Path) -> Result<PathBuf, String> {
    let parent = exe.parent().ok_or_else(|| tr!("cannot establish the installation boundary"))?;
    if platform == Platform::Mac
        && let Some(bundle) = exe.ancestors().find(|p| p.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("app")))
    {
        return Ok(bundle.to_path_buf());
    }
    Ok(parent.to_path_buf())
}

fn forbidden_roots(platform: Platform, env: &impl Fn(&str) -> Option<OsString>, exe: &Path) -> Result<Vec<PathBuf>, String> {
    let mut roots = match platform {
        Platform::Windows => {
            let mut roots = ["APPDATA", "LOCALAPPDATA"]
                .into_iter()
                .map(|key| required_base(env, key).map(|p| p.join("AniCraft")))
                .collect::<Result<Vec<_>, _>>()?;
            // The MSI's default folder (packaging/windows/anicraft.wxs): ProgramFiles64\ArtCraft\
            // AniCraft, which a per-user install makes %LOCALAPPDATA%\Programs\ArtCraft\AniCraft.
            roots.push(required_base(env, "LOCALAPPDATA")?.join("Programs").join("ArtCraft").join("AniCraft")); // i18n-exempt: a path component
            for key in ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"] { // i18n-exempt: environment variable names
                if let Some(base) = env(key).filter(|v| !v.is_empty()) {
                    roots.push(absolute(PathBuf::from(base), key)?.join("ArtCraft").join("AniCraft")); // i18n-exempt: a path component
                }
            }
            roots
        }
        Platform::Linux => {
            let mut roots =
                [("XDG_CONFIG_HOME", ".config"), ("XDG_DATA_HOME", ".local/share"), ("XDG_CACHE_HOME", ".cache"), ("XDG_STATE_HOME", ".local/state")]
                    .into_iter()
                    .map(|(key, fallback)| xdg_base(env, key, fallback).map(|p| p.join("anicraft")))
                    .collect::<Result<Vec<_>, _>>()?;
            roots.extend(["/usr/bin/anicraft", "/usr/local/bin/anicraft", "/opt/anicraft"].map(PathBuf::from));
            // The stable Flatpak keeps its data in its own per-app folder.
            roots.push(required_base(env, "HOME")?.join(".var/app/ai.storyteller.anicraft"));
            roots
        }
        Platform::Mac => {
            let home = required_base(env, "HOME")?;
            let library = home.join("Library"); // i18n-exempt: a path component
            vec![
                library.join("Application Support").join("AniCraft"), // i18n-exempt: a path component
                library.join("Caches").join("AniCraft"), // i18n-exempt: a path component
                library.join("Preferences").join("AniCraft"), // i18n-exempt: a path component
                library.join("Preferences").join("ai.storyteller.anicraft.plist"), // i18n-exempt: a path component
                library.join("Saved Application State").join("ai.storyteller.anicraft.savedState"), // i18n-exempt: a path component
                home.join("Applications").join("AniCraft.app"), // i18n-exempt: a path component
                PathBuf::from("/Applications").join("AniCraft.app"),
            ]
        }
    };
    roots.push(installation_boundary(platform, exe)?);
    roots.into_iter().map(|p| canonical_future(&p)).collect()
}

fn private_subdirectory(root: &Path, name: impl AsRef<Path>, label: &str) -> Result<PathBuf, String> {
    let chosen = canonical_future(&absolute(root.to_path_buf(), tr!("the nightly profile root"))?)?;
    let child = canonical_future(&chosen.join(name))?;
    if child == chosen || !child.starts_with(&chosen) {
        return Err(tr!("the nightly {label} must stay inside its selected profile", label = label));
    }
    Ok(child)
}

/// Every existing entry in the profile, at any depth (prefs, logs, plug-ins, a template category
/// and so on), must resolve inside it, so a link there cannot point Nightly's reads, writes and
/// deletes at another app's data. Links are checked and never followed, so a link cycle cannot
/// loop; a folder that is really inside is walked once, by its own path.
fn entries_stay_inside(root: &Path) -> Result<(), String> {
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let entries = match std::fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && folder.as_path() == root => return Ok(()),
            Err(error) => return Err(tr!("cannot inspect the nightly profile: {e}", e = error)),
        };
        for entry in entries {
            let entry = entry.map_err(|e| tr!("cannot inspect the nightly profile: {e}", e = e))?;
            let kind = entry.file_type().map_err(|e| tr!("cannot inspect the nightly profile: {e}", e = e))?;
            if kind.is_symlink() || folder.as_path() == root {
                let relative = entry.path().strip_prefix(root).map(Path::to_path_buf).unwrap_or_else(|_| entry.file_name().into());
                private_subdirectory(root, relative, tr!("profile entry"))?;
            }
            if kind.is_dir() {
                folders.push(entry.path());
            }
        }
    }
    Ok(())
}

/// Whether `path` is inside a portable copy: [`crate::platform::PORTABLE_MARKER`] beside an
/// AniCraft program, or the `data` folder directly inside such a copy, which is that copy's own
/// settings folder (ADR 0094) and never Nightly's.
fn is_portable_layout(path: &Path, platform: Platform) -> bool {
    let marker = |dir: &Path| std::fs::symlink_metadata(dir.join(crate::platform::PORTABLE_MARKER)).is_ok_and(|m| m.is_file());
    for (depth, ancestor) in path.ancestors().enumerate() {
        // Above the chosen folder itself, a marker only counts beside an AniCraft program, so an
        // unrelated `anicraft-portable` higher up (a user folder, a drive root) refuses nothing.
        if marker(ancestor) && (depth == 0 || ARCHIVE_BINARIES.iter().any(|exe| ancestor.join(exe).is_file())) {
            return true;
        }
    }
    let mut prefix = PathBuf::new();
    for component in path.components() {
        prefix.push(component);
        let is_data = match component {
            Component::Normal(name) if platform.ignores_case() => name.to_string_lossy().eq_ignore_ascii_case(crate::platform::PORTABLE_DATA),
            Component::Normal(name) => name == crate::platform::PORTABLE_DATA,
            _ => false,
        };
        if is_data && prefix.parent().is_some_and(marker) {
            return true;
        }
    }
    false
}

/// The writable hook is the app's own exclusive probe (the one a portable copy uses).
pub fn probe_writable(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let probe = dir.join(".anicraft-write-test");
    let file = std::fs::OpenOptions::new().write(true).create_new(true).open(&probe).map_err(|e| e.to_string())?;
    drop(file);
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

/// Whether the root is a private Nightly place. Anything else is refused before Nightly creates
/// a folder, writes a log line or opens a window: there is no fallback to stable's data.
pub fn validate(
    root: &Path,
    env: impl Fn(&str) -> Option<OsString>,
    exe: Option<&Path>,
    writable: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<PathBuf, String> {
    validate_for(Platform::current()?, root, env, exe, writable)
}

fn validate_for(
    platform: Platform,
    root: &Path,
    env: impl Fn(&str) -> Option<OsString>,
    exe: Option<&Path>,
    writable: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<PathBuf, String> {
    absolute(root.to_path_buf(), tr!("the nightly profile root"))?;
    for key in FOLDER_ENV {
        if env(key).is_some_and(|v| !v.is_empty()) {
            return Err(tr!("nightly-v1 cannot be combined with {name}", name = key));
        }
    }
    let chosen = canonical_future(root)?;
    // These rows are native tar/package launches, not a Flatpak package, and inheriting another
    // app's runtime namespace could point this profile at its socket or data. The Nightly Flatpak
    // is the one instance a Nightly build may run in, and there the profile stays in the app's own
    // private home (`packaging/linux/flatpak-bundle.sh`).
    if platform != Platform::Windows && let Some(id) = env("FLATPAK_ID").filter(|v| !v.is_empty()) {
        if id != crate::platform::APP_ID_NIGHTLY_PACKAGE {
            return Err(tr!("nightly-v1 native packages cannot inherit a Flatpak instance; launch through the native Launcher").into());
        }
        let home = canonical_future(&required_base(&env, "HOME")?)?;
        if !chosen.starts_with(&home) {
            return Err(tr!("the Nightly Flatpak's profile must stay in its own home").into());
        }
    }
    if is_portable_layout(root, platform) || is_portable_layout(&chosen, platform) {
        return Err(tr!("the nightly root must not use a portable AniCraft installation or its data folder").into());
    }
    let exe = absolute(exe.ok_or_else(|| tr!("cannot establish the installation boundary"))?.to_path_buf(), tr!("the executable"))?;
    let forbidden = forbidden_roots(platform, &env, &exe)?;
    if forbidden.iter().any(|stable| overlaps(&chosen, stable, platform)) {
        return Err(tr!("the nightly root must not overlap stable data or an installation").into());
    }
    entries_stay_inside(&chosen)?;
    // Recovery can read, adopt, overwrite and sweep entries: never let its alias leave this
    // private profile, including toward a stable portable data folder not in our known roots.
    private_subdirectory(&chosen, "Recovery", tr!("recovery folder"))?; // i18n-exempt: the profile folder name
    let mut create = std::fs::DirBuilder::new();
    create.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        create.mode(0o700);
    }
    create.create(&chosen).map_err(|e| tr!("cannot create the nightly profile: {e}", e = e))?;
    let confirmed = std::fs::canonicalize(&chosen).map_err(|e| tr!("cannot resolve the created nightly profile: {e}", e = e))?;
    if confirmed != chosen || forbidden.iter().any(|stable| overlaps(&confirmed, stable, platform)) {
        return Err(tr!("the nightly profile changed while it was being validated").into());
    }
    private_subdirectory(&confirmed, "Recovery", tr!("recovery folder"))?; // i18n-exempt: the profile folder name
    entries_stay_inside(&confirmed)?;
    writable(&confirmed).map_err(|e| tr!("the nightly profile is not writable: {e}", e = e))?;
    Ok(confirmed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture {
        root: PathBuf,
        env: BTreeMap<String, OsString>,
        exe: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = loop {
                let candidate =
                    std::env::temp_dir().join(format!("anicraft-profile-policy-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
                match std::fs::create_dir(&candidate) {
                    Ok(()) => break std::fs::canonicalize(candidate).unwrap(),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("cannot create owned fixture: {error}"),
                }
            };
            let home = root.join("home");
            let exe = root.join("install/anicraft");
            std::fs::create_dir_all(&home).unwrap();
            std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
            std::fs::write(&exe, b"fixture executable, never run").unwrap();
            let env = [("HOME", home), ("APPDATA", root.join("roaming")), ("LOCALAPPDATA", root.join("local"))]
                .into_iter()
                .map(|(key, value)| (key.into(), value.into_os_string()))
                .collect();
            Self { root, env, exe }
        }

        fn get(&self, key: &str) -> Option<OsString> {
            self.env.get(key).cloned()
        }

        fn check(&self, platform: Platform, root: &Path) -> Result<PathBuf, String> {
            validate_for(platform, root, |key| self.get(key), Some(&self.exe), probe)
        }

        fn home(&self) -> PathBuf {
            PathBuf::from(self.get("HOME").unwrap())
        }
    }

    /// An exclusive probe like the app's, without global state.
    fn probe(dir: &Path) -> Result<(), String> {
        let probe = dir.join("exclusive-test-probe");
        let file = std::fs::OpenOptions::new().write(true).create_new(true).open(&probe).map_err(|e| e.to_string())?;
        drop(file);
        std::fs::remove_file(probe).map_err(|e| e.to_string())
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            // Only the directory successfully created by this fixture is owned for removal.
            std::fs::remove_dir_all(&self.root).unwrap();
        }
    }

    #[test]
    fn portable_settings_from_another_install_are_refused_before_any_probe() {
        let fixture = Fixture::new();
        let stable_install = fixture.root.join("stable portable install");
        let stable_data = stable_install.join(crate::platform::PORTABLE_DATA);
        std::fs::create_dir_all(&stable_data).unwrap();
        std::fs::write(stable_install.join(crate::platform::PORTABLE_MARKER), b"owned portable fixture").unwrap();
        std::fs::write(stable_data.join("prefs.json"), b"stable settings must not change").unwrap();
        for platform in [Platform::Windows, Platform::Linux, Platform::Mac] {
            for candidate in [&stable_data, &stable_data.join("nested nightly")] {
                let probed = std::cell::Cell::new(false);
                let result = validate_for(
                    platform,
                    candidate,
                    |key| fixture.get(key),
                    Some(&fixture.exe),
                    |_| {
                        probed.set(true);
                        Ok(())
                    },
                );
                assert!(result.is_err(), "a different portable install is not a private Nightly profile");
                assert!(!probed.get(), "reject portable data before a writable probe");
                assert!(!stable_data.join("nested nightly").exists());
                assert_eq!(std::fs::read(stable_data.join("prefs.json")).unwrap(), b"stable settings must not change");
                assert_eq!(std::fs::read_dir(&stable_data).unwrap().count(), 1);
            }
        }
        let private = fixture.root.join("AniCraftData-backup-name/nightly");
        assert!(fixture.check(Platform::Linux, &private).is_ok(), "a merely similar name is not a portable data component");
    }

    #[cfg(unix)]
    #[test]
    fn recovery_aliases_cannot_redirect_to_stable_missing_or_profile_root_data() {
        let fixture = Fixture::new();
        let stable = fixture.home().join(".local/state/anicraft/Recovery");
        std::fs::create_dir_all(&stable).unwrap();
        std::fs::write(stable.join("stable-autosave"), b"stable recovery bytes").unwrap();
        for (index, target) in [stable.clone(), fixture.root.join("missing recovery target")].into_iter().enumerate() {
            let private = fixture.root.join(format!("nightly-{index}"));
            std::fs::create_dir(&private).unwrap();
            std::os::unix::fs::symlink(&target, private.join("Recovery")).unwrap();
            let probed = std::cell::Cell::new(false);
            let result = validate_for(
                Platform::Linux,
                &private,
                |key| fixture.get(key),
                Some(&fixture.exe),
                |_| {
                    probed.set(true);
                    Ok(())
                },
            );
            assert!(result.is_err(), "Recovery must not escape the private profile or follow a dangling alias");
            assert!(!probed.get());
        }
        let private = fixture.root.join("nightly-self");
        std::fs::create_dir(&private).unwrap();
        std::os::unix::fs::symlink(&private, private.join("Recovery")).unwrap();
        assert!(fixture.check(Platform::Linux, &private).is_err(), "Recovery cannot treat all profile settings as recovery entries");
        assert_eq!(std::fs::read(stable.join("stable-autosave")).unwrap(), b"stable recovery bytes");
        assert_eq!(std::fs::read_dir(stable).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn recovery_alias_to_a_private_subfolder_keeps_the_profile_usable() {
        let fixture = Fixture::new();
        let private = fixture.root.join("nightly-private");
        let recovery = private.join("private-recovery");
        std::fs::create_dir_all(&recovery).unwrap();
        std::os::unix::fs::symlink(&recovery, private.join("Recovery")).unwrap();
        assert!(fixture.check(Platform::Linux, &private).is_ok());
        assert_eq!(std::fs::canonicalize(private.join("Recovery")).unwrap(), recovery);
    }

    #[test]
    fn overlap_check_handles_ancestor_descendant_and_prefix_lookalikes() {
        let fixture = Fixture::new();
        let root = fixture.home().join("nightly/profiles/anicraft");
        for platform in [Platform::Windows, Platform::Linux, Platform::Mac] {
            assert!(overlaps(&root, &root.join("Recovery"), platform));
            assert!(overlaps(&root.join("Recovery"), &root, platform));
            assert!(!overlaps(&root, &root.with_file_name("anicraft-other"), platform));
        }
    }

    #[test]
    fn linux_fences_app_xdg_subtrees_but_allows_launcher_sibling() {
        let mut fixture = Fixture::new();
        for key in ["XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME"] {
            fixture.env.insert(key.into(), fixture.root.join(key).into_os_string());
        }
        for key in ["XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME"] {
            let stable = PathBuf::from(fixture.get(key).unwrap()).join("anicraft");
            std::fs::create_dir_all(&stable).unwrap();
            std::fs::write(stable.join("stable-sentinel"), b"stable bytes").unwrap();
            for candidate in [stable.clone(), stable.join("nightly"), stable.parent().unwrap().to_path_buf()] {
                assert!(fixture.check(Platform::Linux, &candidate).unwrap_err().contains("overlap"));
            }
            assert_eq!(std::fs::read(stable.join("stable-sentinel")).unwrap(), b"stable bytes");
        }
        let data = PathBuf::from(fixture.get("XDG_DATA_HOME").unwrap());
        let nightly = data.join("ArtCraftLauncher/nightly/profiles/anicraft");
        assert_eq!(fixture.check(Platform::Linux, &nightly).unwrap(), nightly);
        assert_eq!(std::fs::read_dir(nightly).unwrap().count(), 0);
    }

    #[test]
    fn mac_fences_only_app_library_subtrees_and_the_entire_bundle() {
        let mut fixture = Fixture::new();
        fixture.exe = fixture.root.join("install/AniCraft Nightly.app/Contents/MacOS/AniCraft");
        std::fs::create_dir_all(fixture.exe.parent().unwrap()).unwrap();
        std::fs::write(&fixture.exe, b"fixture executable, never run").unwrap();
        let library = fixture.home().join("Library");
        for suffix in
            ["Application Support/AniCraft", "Caches/AniCraft", "Preferences/AniCraft", "Saved Application State/ai.storyteller.anicraft.savedState"]
        {
            let stable = library.join(suffix);
            assert!(fixture.check(Platform::Mac, &stable.join("nightly")).unwrap_err().contains("overlap"));
            assert!(!stable.exists());
        }
        let inside_resources = fixture.exe.parent().unwrap().parent().unwrap().join("Resources/profile");
        assert!(fixture.check(Platform::Mac, &inside_resources).unwrap_err().contains("overlap"));
        assert!(!inside_resources.exists());
        let valid = library.join("Application Support/ArtCraftLauncher/nightly/profiles/anicraft");
        assert_eq!(fixture.check(Platform::Mac, &valid).unwrap(), valid);
    }

    #[test]
    fn windows_roaming_local_and_install_fences_remain_active() {
        let fixture = Fixture::new();
        for key in ["APPDATA", "LOCALAPPDATA"] {
            let stable = PathBuf::from(fixture.get(key).unwrap()).join("AniCraft");
            assert!(fixture.check(Platform::Windows, &stable.join("nightly")).unwrap_err().contains("overlap"));
            assert!(!stable.exists());
        }
        assert!(fixture.check(Platform::Windows, &fixture.exe.parent().unwrap().join("profile")).is_err());
        assert!(fixture.check(Platform::Windows, &fixture.root.join("Launcher/profiles/anicraft")).is_ok());
    }

    #[test]
    fn windows_default_msi_install_folders_are_fenced() {
        let mut fixture = Fixture::new();
        let local = PathBuf::from(fixture.get("LOCALAPPDATA").unwrap());
        let per_user = local.join("Programs").join("ArtCraft").join("AniCraft");
        assert!(fixture.check(Platform::Windows, &per_user.join("nightly")).unwrap_err().contains("overlap"));
        for key in ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"] {
            let base = fixture.root.join(format!("machine {key}"));
            fixture.env.insert(key.into(), base.clone().into_os_string());
            let machine = base.join("ArtCraft").join("AniCraft");
            assert!(fixture.check(Platform::Windows, &machine.join("nightly")).unwrap_err().contains("overlap"));
            assert!(!machine.exists());
        }
        let sibling = local.join("Programs").join("ArtCraft").join("ArtCraftLauncher").join("nightly").join("anicraft");
        assert!(fixture.check(Platform::Windows, &sibling).is_ok(), "the Launcher's own Programs sibling is not AniCraft's install");
    }

    #[test]
    fn folders_under_a_stable_portable_copy_are_refused_before_any_probe() {
        let fixture = Fixture::new();
        let portable = fixture.root.join("D/AC");
        std::fs::create_dir_all(&portable).unwrap();
        std::fs::write(portable.join(crate::platform::PORTABLE_MARKER), b"stable portable copy").unwrap();
        std::fs::write(portable.join("anicraft.exe"), b"stable portable executable, never run").unwrap();
        for platform in [Platform::Windows, Platform::Linux, Platform::Mac] {
            for candidate in [portable.join("nightly"), portable.join("deeper/nightly")] {
                let probed = std::cell::Cell::new(false);
                let result = validate_for(
                    platform,
                    &candidate,
                    |key| fixture.get(key),
                    Some(&fixture.exe),
                    |_| {
                        probed.set(true);
                        Ok(())
                    },
                );
                assert!(result.unwrap_err().contains("portable"));
                assert!(!probed.get());
                assert!(!candidate.exists());
            }
        }
        // An `anicraft-portable` that no AniCraft executable sits beside is someone else's file.
        let unrelated = fixture.root.join("user folder");
        std::fs::create_dir_all(&unrelated).unwrap();
        std::fs::write(unrelated.join(crate::platform::PORTABLE_MARKER), b"another app's marker").unwrap();
        assert!(fixture.check(Platform::Windows, &unrelated.join("Launcher/nightly/anicraft")).is_ok());
    }

    #[test]
    fn linux_fences_the_stable_flatpak_data_folder() {
        let fixture = Fixture::new();
        let flatpak = fixture.home().join(".var/app/ai.storyteller.anicraft");
        for candidate in [flatpak.clone(), flatpak.join("data/nightly")] {
            assert!(fixture.check(Platform::Linux, &candidate).unwrap_err().contains("overlap"));
        }
        assert!(!flatpak.exists());
        assert!(fixture.check(Platform::Linux, &fixture.home().join(".var/app/ai.storyteller.launcher/nightly")).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn any_profile_entry_alias_must_stay_inside_the_profile() {
        let fixture = Fixture::new();
        let stable = fixture.home().join(".config/anicraft/Templates");
        std::fs::create_dir_all(&stable).unwrap();
        std::fs::write(stable.join("stable-template"), b"stable template bytes").unwrap();
        for name in ["Templates", "Logs", "prefs.json"] {
            let private = fixture.root.join(format!("nightly-{name}"));
            std::fs::create_dir(&private).unwrap();
            std::os::unix::fs::symlink(&stable, private.join(name)).unwrap();
            let probed = std::cell::Cell::new(false);
            let result = validate_for(
                Platform::Linux,
                &private,
                |key| fixture.get(key),
                Some(&fixture.exe),
                |_| {
                    probed.set(true);
                    Ok(())
                },
            );
            assert!(result.is_err(), "{name} must not lead outside the profile");
            assert!(!probed.get());
        }
        // A link one level down is as dangerous as one in the profile's own folder: Templates is a
        // real folder here, but its category leads into the stable app's templates.
        let private = fixture.root.join("nightly-nested");
        std::fs::create_dir_all(private.join("Templates")).unwrap();
        std::os::unix::fs::symlink(&stable, private.join("Templates").join("Characters")).unwrap();
        let probed = std::cell::Cell::new(false);
        let result = validate_for(
            Platform::Linux,
            &private,
            |key| fixture.get(key),
            Some(&fixture.exe),
            |_| {
                probed.set(true);
                Ok(())
            },
        );
        assert!(result.is_err(), "a nested link must not lead outside the profile");
        assert!(!probed.get());
        let private = fixture.root.join("nightly-nested-inside");
        std::fs::create_dir_all(private.join("Templates/real")).unwrap();
        std::os::unix::fs::symlink(private.join("Templates/real"), private.join("Templates/Characters")).unwrap();
        std::os::unix::fs::symlink(&private, private.join("Templates/real/loop")).unwrap();
        assert!(fixture.check(Platform::Linux, &private).is_err(), "a link back to the profile's own folder is refused, not looped on");
        std::fs::remove_file(private.join("Templates/real/loop")).unwrap();
        assert!(fixture.check(Platform::Linux, &private).is_ok(), "a nested link that stays inside is fine");
        let private = fixture.root.join("nightly-inside");
        std::fs::create_dir_all(private.join("real-templates")).unwrap();
        std::fs::write(private.join("prefs.json"), b"{}").unwrap();
        std::os::unix::fs::symlink(private.join("real-templates"), private.join("Templates")).unwrap();
        assert!(fixture.check(Platform::Linux, &private).is_ok(), "regular files and links that stay inside are fine");
        assert_eq!(std::fs::read(stable.join("stable-template")).unwrap(), b"stable template bytes");
    }

    #[test]
    fn inherited_folder_overrides_and_non_native_sandbox_are_refused_before_creation() {
        let mut fixture = Fixture::new();
        let candidate = fixture.home().join("nightly/profiles/anicraft");
        fixture.env.insert("XDG_CACHE_HOME".into(), "relative/cache".into());
        assert!(fixture.check(Platform::Linux, &candidate).unwrap_err().contains("absolute"));
        fixture.env.remove("XDG_CACHE_HOME");
        for key in FOLDER_ENV {
            fixture.env.insert(key.into(), fixture.home().join("moved").into_os_string());
            assert!(fixture.check(Platform::Linux, &candidate).unwrap_err().contains(key), "{key}");
            fixture.env.remove(key);
        }
        fixture.env.insert("FLATPAK_ID".into(), "ai.storyteller.anicraft".into());
        assert!(fixture.check(Platform::Linux, &candidate).unwrap_err().contains("Flatpak"));
        // The Nightly Flatpak itself is a supported format, as long as its profile stays in the
        // sandbox's own home; anything else keeps the inheritance refusal.
        fixture.env.insert("FLATPAK_ID".into(), crate::platform::APP_ID_NIGHTLY_PACKAGE.into());
        let in_sandbox = fixture.home().join("sandbox-home/anicraft-nightly/profile");
        assert_eq!(fixture.check(Platform::Linux, &in_sandbox).unwrap(), std::fs::canonicalize(&in_sandbox).unwrap());
        let outside = fixture.root.join("outside-the-sandbox").join("anicraft-nightly");
        let probed = std::cell::Cell::new(false);
        let result = validate_for(Platform::Linux, &outside, |key| fixture.get(key), Some(&fixture.exe), |_| {
            probed.set(true);
            Ok(())
        });
        assert!(result.unwrap_err().contains("own home"));
        assert!(!probed.get() && !outside.exists());
        fixture.env.remove("FLATPAK_ID");
        fixture.env.remove("HOME");
        assert!(fixture.check(Platform::Linux, &candidate).unwrap_err().contains("HOME"));
        assert!(!candidate.exists());
    }

    #[test]
    fn non_directory_and_failed_write_probe_never_fall_back_to_stable() {
        let fixture = Fixture::new();
        let file = fixture.home().join("not-a-directory");
        std::fs::write(&file, b"a file where the profile should be").unwrap();
        assert!(fixture.check(Platform::Linux, &file).is_err());
        let candidate = fixture.home().join("nightly-profile");
        let result = validate_for(Platform::Linux, &candidate, |key| fixture.get(key), Some(&fixture.exe), |_| Err("read-only medium".into()));
        assert!(result.unwrap_err().contains("not writable"));
        assert!(candidate.is_dir(), "no fallback folder is created elsewhere");
        assert!(!fixture.home().join(".config/anicraft").exists(), "stable's settings folder is never made");
    }

    #[test]
    fn windows_and_macos_compare_like_paths_case_insensitively() {
        let fixture = Fixture::new();
        for (platform, stable) in [
            (Platform::Windows, PathBuf::from(fixture.get("APPDATA").unwrap()).join("AniCraft")),
            (Platform::Mac, fixture.home().join("Library").join("Application Support").join("AniCraft")),
        ] {
            let shouty = stable.with_file_name("ANICRAFT");
            assert!(fixture.check(platform, &shouty.join("Nightly")).unwrap_err().contains("overlap"), "{shouty:?}");
        }
        // Linux paths are case-sensitive: a differently cased folder is another folder.
        let linux = fixture.home().join(".config/AniCraft/nightly");
        assert!(fixture.check(Platform::Linux, &linux).is_ok());
    }
}
