//! Launcher-owned nightly-v1 bootstrap. Selected before logging, updates, fonts or saved state.
//! Stable launches retain their original arguments and directory policy. No environment mutation.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

pub const VERSION: &str = "nightly-v1";

/// Where a distribution wrapper names the profile root when it cannot add arguments in front of
/// the person's own. Every format this app ships adds the two arguments through
/// `packaging/linux/nightly-launch.sh` instead, so nothing here uses the variable today; it stays
/// as the one door for a packager whose launcher can only set an environment, is read only by a
/// nightly build, is validated exactly like the Launcher's root, and may not be combined with the
/// Launcher's two arguments.
pub const ROOT_ENV: &str = "ANICRAFT_PROFILE_ROOT";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Profile {
    Stable,
    NightlyV1(PathBuf),
}

impl Profile {
    pub fn is_nightly(&self) -> bool {
        matches!(self, Self::NightlyV1(_))
    }

    pub fn root(&self) -> Option<&Path> {
        match self {
            Self::Stable => None,
            Self::NightlyV1(root) => Some(root),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Launch {
    pub profile: Profile,
    pub args: Vec<OsString>,
}

static PROFILE: OnceLock<Profile> = OnceLock::new();

pub fn current() -> &'static Profile {
    PROFILE.get().unwrap_or(&Profile::Stable)
}

pub fn install(profile: Profile) -> Result<(), String> {
    PROFILE.set(profile).map_err(|_| tr!("the launch profile was already selected").into())
}

/// Recognize only the agreed literal selector/root pair before `--`. Everything after it is
/// a file, even when it looks like a profile flag. No filesystem or startup hook runs here.
pub fn parse(args: Vec<OsString>) -> Result<Launch, String> {
    let mut selector = None;
    let mut root = None;
    let mut other = Vec::new();
    let mut files = Vec::new();
    let mut iter = args.iter();
    let mut delimiter = false;
    while let Some(arg) = iter.next() {
        if arg == "--" {
            delimiter = true;
            files.extend(iter.cloned());
            break;
        }
        if arg == "--launch-profile" || arg == "--profile-root" {
            let slot = if arg == "--launch-profile" { &mut selector } else { &mut root };
            if slot.is_some() {
                return Err(tr!("the profile selector and root must each occur exactly once").into());
            }
            let value = iter.next().filter(|v| !v.is_empty()).ok_or_else(|| tr!("the profile selector/root needs a value").to_string())?;
            if value.to_string_lossy().starts_with("--") {
                return Err(tr!("the profile selector/root needs a value").into());
            }
            *slot = Some(value.clone());
        } else if arg.to_string_lossy().starts_with("--launch-profile=") || arg.to_string_lossy().starts_with("--profile-root=") {
            return Err(tr!("the nightly protocol requires separate literal selector/root argument tokens").into());
        } else {
            other.push(arg.clone());
        }
    }
    match (selector, root) {
        (None, None) => Ok(Launch { profile: Profile::Stable, args }),
        (Some(version), Some(root)) if version == VERSION => {
            if !delimiter || !other.is_empty() {
                return Err(tr!("nightly-v1 accepts only its selector/root followed by -- and file names").into());
            }
            let root = nightly_root(&root)?;
            Ok(Launch { profile: Profile::NightlyV1(root), args: std::iter::once(OsString::from("--")).chain(files).collect() })
        }
        (Some(_), Some(_)) => Err(tr!("unsupported launch profile (expected nightly-v1)").into()),
        _ => Err(tr!("--launch-profile and --profile-root must be supplied together").into()),
    }
}

/// A root is lexical here: absolute, no `..`. Whether it is a safe place for Nightly's data is
/// [`crate::profile_root::validate`]'s decision, taken before the first startup effect.
fn nightly_root(value: &std::ffi::OsStr) -> Result<PathBuf, String> {
    let root = PathBuf::from(value);
    if !root.is_absolute() || root.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(tr!("the nightly profile root must be absolute and contain no parent traversal").into());
    }
    Ok(root)
}

/// The Launcher's version check: the selector/root pair followed by exactly `--version` (or `-V`),
/// answered like a plain version query. Only [`parse`]'s lexical checks apply; the root is never
/// validated or touched, and `-- --version` stays a file name.
pub fn version_query(args: &[OsString]) -> bool {
    let Some((last, pair)) = args.split_last() else { return false };
    if last != "--version" && last != "-V" {
        return false;
    }
    let lexical = pair.iter().cloned().chain(std::iter::once(OsString::from("--"))).collect();
    matches!(parse(lexical), Ok(Launch { profile: Profile::NightlyV1(_), args }) if args.len() == 1)
}

/// [`parse`], plus the wrapper's root for a distribution package that cannot add arguments. A
/// build passes `wrapper_root` only for the nightly channel, so a stable build never sees it.
pub fn parse_startup(args: Vec<OsString>, wrapper_root: Option<OsString>) -> Result<Launch, String> {
    // Only the tokens before `--`: after it everything is a file name, even `--profile-root`.
    let launcher_form = args.iter().take_while(|a| *a != "--").any(|a| {
        a == "--launch-profile"
            || a == "--profile-root"
            || a.to_string_lossy().starts_with("--launch-profile=")
            || a.to_string_lossy().starts_with("--profile-root=")
    });
    let wrapper = wrapper_root.filter(|v| !v.is_empty());
    match (launcher_form, wrapper) {
        (true, Some(_)) => Err(tr!("nightly-v1 takes its profile from the Launcher or {name}, not both", name = ROOT_ENV)),
        (true, None) => parse(args),
        (false, Some(root)) => Ok(Launch { profile: Profile::NightlyV1(nightly_root(&root)?), args }),
        // Stable's argument order and parser remain unchanged.
        (false, None) => Ok(Launch { profile: Profile::Stable, args }),
    }
}

/// Validation finishes before the caller's first startup effect. Stable never calls the nightly
/// root validator, so its old portable/config resolution stays unchanged.
pub fn prepare(mut launch: Launch, validate: impl FnOnce(&Path) -> Result<PathBuf, String>) -> Result<Launch, String> {
    if let Profile::NightlyV1(root) = &launch.profile {
        launch.profile = Profile::NightlyV1(validate(root)?);
    }
    Ok(launch)
}

/// Resolve the selected private root before the caller's first startup effect.
pub fn validate_root(root: &Path, env: impl Fn(&str) -> Option<OsString>, exe: Option<&Path>) -> Result<PathBuf, String> {
    crate::profile_root::validate(root, env, exe, crate::profile_root::probe_writable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }
    fn absolute() -> &'static str {
        if cfg!(windows) { r"C:\NightlyData\profiles\anicraft" } else { "/nightly-data/profiles/anicraft" }
    }
    fn nightly() -> Vec<OsString> {
        args(&["--launch-profile", VERSION, "--profile-root", absolute(), "--"])
    }

    #[test]
    fn stable_arguments_and_directory_policy_are_unchanged() {
        for argv in [args(&[]), args(&["walk.anicraft"]), args(&["--", "--launch-profile", "shot.png"])] {
            let launch = prepare(parse(argv.clone()).unwrap(), |_| panic!("stable must never validate a nightly root")).unwrap();
            assert_eq!(launch, Launch { profile: Profile::Stable, args: argv });
        }
        // Without the wrapper variable a start is stable, whatever it was given.
        let launch = parse_startup(args(&["walk.anicraft"]), None).unwrap();
        assert_eq!((launch.profile, launch.args), (Profile::Stable, args(&["walk.anicraft"])));
    }

    #[test]
    fn exact_version_and_root_are_parsed_without_reinterpreting_file_names() {
        let mut argv = nightly();
        argv.extend(args(&["--profile-root", "scene with spaces.anicraft"]));
        let launch = parse(argv).unwrap();
        assert_eq!(launch.profile, Profile::NightlyV1(PathBuf::from(absolute())));
        assert_eq!(launch.args, args(&["--", "--profile-root", "scene with spaces.anicraft"]));
    }

    #[test]
    fn missing_duplicate_unknown_and_conflicting_profile_arguments_are_refused() {
        let equals_form = format!("--profile-root={}", absolute());
        for argv in [
            args(&["--launch-profile", VERSION]),
            args(&["--profile-root", absolute()]),
            args(&["--launch-profile", "future-v2", "--profile-root", absolute(), "--"]),
            args(&["--launch-profile", VERSION, "--launch-profile", VERSION, "--profile-root", absolute(), "--"]),
            args(&["--launch-profile", VERSION, "--profile-root", absolute(), "--profile-root", absolute(), "--"]),
            args(&["--launch-profile", VERSION, "--profile-root", "relative", "--"]),
            args(&["--launch-profile", VERSION, "--profile-root", "", "--"]),
            args(&["--launch-profile", VERSION, "--profile-root", "../escaping", "--"]),
            args(&["--launch-profile", VERSION, "--profile-root", absolute(), "--new-instance", "--"]),
            args(&["--launch-profile", VERSION, "--profile-root", absolute()]),
            args(&[&equals_form[..], "--launch-profile", VERSION, "--"]),
        ] {
            assert!(parse(argv).is_err());
        }
    }

    #[test]
    fn the_launcher_version_check_is_a_version_query_only_in_its_exact_form() {
        let pair = &["--launch-profile", VERSION, "--profile-root", absolute()];
        let reversed = &["--profile-root", absolute(), "--launch-profile", VERSION];
        for flag in ["--version", "-V"] {
            for form in [pair, reversed] {
                let mut argv = args(form);
                argv.push(flag.into());
                assert!(version_query(&argv), "{argv:?}");
            }
        }
        for argv in [
            args(&["--version"]),
            args(&["--launch-profile", VERSION, "--profile-root", absolute(), "--", "--version"]),
            args(&["--launch-profile", VERSION, "--profile-root", absolute(), "--version", "--"]),
            args(&["--launch-profile", VERSION, "--profile-root", absolute(), "--screenshot", "--version"]),
            args(&["--launch-profile", VERSION, "--profile-root", absolute(), "--version", "--version"]),
            args(&["--version", "--launch-profile", VERSION, "--profile-root", absolute()]),
            args(&["--launch-profile", "future-v2", "--profile-root", absolute(), "--version"]),
            args(&["--launch-profile", VERSION, "--profile-root", "relative", "--version"]),
            args(&["--launch-profile", VERSION, "--version"]),
            args(&["--launch-profile", VERSION, "--profile-root", absolute(), "--help"]),
        ] {
            assert!(!version_query(&argv), "{argv:?}");
        }
        // Outside the query the strict protocol still refuses the version flag.
        assert!(parse(args(&["--launch-profile", VERSION, "--profile-root", absolute(), "--version"])).is_err());
    }

    #[test]
    fn the_wrapper_root_is_the_same_profile_and_cannot_be_mixed_with_the_launcher_form() {
        let launch = parse_startup(args(&["walk cycle.anicraft"]), Some(absolute().into())).unwrap();
        assert_eq!(launch.profile, Profile::NightlyV1(PathBuf::from(absolute())));
        assert_eq!(launch.args, args(&["walk cycle.anicraft"]));
        // An empty value is no value, as a shell's `VAR=` sets it.
        assert_eq!(parse_startup(args(&[]), Some(OsString::new())).unwrap().profile, Profile::Stable);
        // After `--` every token is a file name, so a file called `--profile-root` is not the Launcher's.
        let after_delimiter = parse_startup(args(&["--", "--profile-root"]), Some(absolute().into())).unwrap();
        assert_eq!((after_delimiter.profile, after_delimiter.args), (Profile::NightlyV1(PathBuf::from(absolute())), args(&["--", "--profile-root"])));
        // Both at once is ambiguous about which root the window would use, so neither is used.
        assert!(parse_startup(nightly(), Some(absolute().into())).is_err());
        assert!(parse_startup(args(&[]), Some("relative".into())).is_err());
        assert!(parse_startup(args(&[]), Some("../escaping".into())).is_err());
        // One half of the Launcher's pair on its own is refused (a reserved token, never a file),
        // and so is a profile the build cannot honour.
        assert!(parse_startup(args(&["--profile-root"]), None).is_err());
        assert!(parse_startup(args(&["--launch-profile", "future-v2", "--profile-root", absolute(), "--"]), None).is_err());
    }

    #[test]
    fn failed_root_validation_never_reaches_startup_or_falls_back_to_stable() {
        let reached = Cell::new(false);
        let result = prepare(parse(nightly()).unwrap(), |_| Err("read-only or unsafe profile".into())).map(|_| reached.set(true));
        assert!(result.is_err());
        assert!(!reached.get());
    }

    #[test]
    fn successful_root_validation_keeps_the_persistent_root_not_the_binary_tag() {
        let checked = PathBuf::from(absolute()).join("checked");
        let launch = prepare(parse(nightly()).unwrap(), |_| Ok(checked.clone())).unwrap();
        assert_eq!(launch.profile.root(), Some(checked.as_path()));
        assert!(launch.profile.is_nightly());
    }
}
