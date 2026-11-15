//! Where the garage lives when the user does not say (DR-0002 G1, Q4).
//!
//! The resolver lives in the front end, not in `garage` (G9). It only works
//! out a path: it reads no file and creates no directory. The environment is
//! passed in, so tests never look at the real one.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Environment variable naming the garage directory. `--garage-dir` wins
/// over it.
pub const OVERRIDE_VAR: &str = "DIAGWIRE_GARAGE_DIR";
/// The garage's main file inside its directory. SQLite keeps the `-wal` and
/// `-shm` sidecars next to it.
pub const FILE_NAME: &str = "garage.sqlite3";
const APP_DIR: &str = "diagwire";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    MacOs,
    /// Linux and every other Unix: the XDG base directory rules.
    Unix,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(windows) {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Unix
        }
    }
}

/// Who chose the directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChosenBy {
    Flag,
    Environment,
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GaragePath {
    pub dir: PathBuf,
    pub chosen_by: ChosenBy,
}

impl GaragePath {
    pub fn file(&self) -> PathBuf {
        self.dir.join(FILE_NAME)
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("{0} is not set, so there is no default garage location; pass --garage-dir or set {OVERRIDE_VAR}")]
    Unset(&'static str),
    #[error("{0} is not an absolute path, so it cannot locate the garage; pass --garage-dir or set {OVERRIDE_VAR}")]
    Relative(&'static str),
}

/// Resolves the garage directory: the flag, then [`OVERRIDE_VAR`], then the
/// platform's per-user local data directory. A directory the user names is
/// taken as given. An empty variable counts as unset.
pub fn resolve(
    flag: Option<&Path>,
    platform: Platform,
    env: impl Fn(&str) -> Option<OsString>,
) -> Result<GaragePath, Error> {
    let set = |name: &str| env(name).filter(|value| !value.is_empty()).map(PathBuf::from);
    // A base directory taken from the environment must be absolute: a
    // relative one would put the garage wherever the command happened to run.
    let base = |name: &'static str| match set(name) {
        Some(path) if is_absolute(&path, platform) => Ok(Some(path)),
        Some(_) => Err(Error::Relative(name)),
        None => Ok(None),
    };

    if let Some(dir) = flag {
        return Ok(GaragePath {
            dir: dir.to_path_buf(),
            chosen_by: ChosenBy::Flag,
        });
    }
    if let Some(dir) = set(OVERRIDE_VAR) {
        return Ok(GaragePath {
            dir,
            chosen_by: ChosenBy::Environment,
        });
    }

    let data_dir = match platform {
        // Local, never Roaming (`APPDATA`): a roaming profile may be synced
        // off the machine.
        Platform::Windows => base("LOCALAPPDATA")?.ok_or(Error::Unset("LOCALAPPDATA"))?,
        Platform::MacOs => base("HOME")?
            .ok_or(Error::Unset("HOME"))?
            .join("Library")
            .join("Application Support"),
        Platform::Unix => match base("XDG_DATA_HOME")? {
            Some(dir) => dir,
            None => base("HOME")?.ok_or(Error::Unset("HOME"))?.join(".local").join("share"),
        },
    };
    Ok(GaragePath {
        dir: data_dir.join(APP_DIR),
        chosen_by: ChosenBy::Default,
    })
}

/// `Path::is_absolute` answers for the host. This answers for `platform`,
/// so every platform's rule can be tested on any host.
fn is_absolute(path: &Path, platform: Platform) -> bool {
    let text = path.to_string_lossy();
    match platform {
        Platform::MacOs | Platform::Unix => text.starts_with('/'),
        Platform::Windows => {
            let bytes = text.as_bytes();
            let drive = bytes.len() >= 3
                && bytes[0].is_ascii_alphabetic()
                && bytes[1] == b':'
                && matches!(bytes[2], b'\\' | b'/');
            drive || text.starts_with(r"\\")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake environment. Tests never read the real one.
    fn env(vars: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
        move |name| vars.iter().find(|(key, _)| *key == name).map(|(_, value)| OsString::from(value))
    }

    fn default_dir(platform: Platform, vars: &'static [(&'static str, &'static str)]) -> Result<PathBuf, Error> {
        let resolved = resolve(None, platform, env(vars))?;
        assert_eq!(resolved.chosen_by, ChosenBy::Default);
        Ok(resolved.dir)
    }

    #[test]
    fn linux_uses_xdg_data_home_when_set() {
        let dir = default_dir(Platform::Unix, &[("XDG_DATA_HOME", "/data/xdg"), ("HOME", "/home/u")]);
        assert_eq!(dir, Ok(PathBuf::from("/data/xdg").join("diagwire")));
    }

    #[test]
    fn linux_falls_back_to_local_share_when_xdg_is_unset_or_empty() {
        let expected = PathBuf::from("/home/u").join(".local").join("share").join("diagwire");
        assert_eq!(default_dir(Platform::Unix, &[("HOME", "/home/u")]), Ok(expected.clone()));
        assert_eq!(
            default_dir(Platform::Unix, &[("XDG_DATA_HOME", ""), ("HOME", "/home/u")]),
            Ok(expected)
        );
    }

    #[test]
    fn macos_uses_application_support() {
        // XDG is not consulted on macOS.
        let dir = default_dir(Platform::MacOs, &[("XDG_DATA_HOME", "/data/xdg"), ("HOME", "/Users/u")]);
        let expected = PathBuf::from("/Users/u")
            .join("Library")
            .join("Application Support")
            .join("diagwire");
        assert_eq!(dir, Ok(expected));
    }

    #[test]
    fn windows_uses_local_app_data_and_never_roaming() {
        let local = r"C:\Users\u\AppData\Local";
        let dir = default_dir(
            Platform::Windows,
            &[("LOCALAPPDATA", r"C:\Users\u\AppData\Local"), ("APPDATA", r"C:\Users\u\AppData\Roaming")],
        );
        assert_eq!(dir, Ok(PathBuf::from(local).join("diagwire")));

        // With only the roaming directory known there is no default.
        let dir = default_dir(Platform::Windows, &[("APPDATA", r"C:\Users\u\AppData\Roaming")]);
        assert_eq!(dir, Err(Error::Unset("LOCALAPPDATA")));
    }

    #[test]
    fn no_default_without_a_home() {
        assert_eq!(default_dir(Platform::Unix, &[]), Err(Error::Unset("HOME")));
        assert_eq!(default_dir(Platform::Unix, &[("HOME", "")]), Err(Error::Unset("HOME")));
        assert_eq!(default_dir(Platform::MacOs, &[]), Err(Error::Unset("HOME")));
        assert_eq!(default_dir(Platform::Windows, &[("LOCALAPPDATA", "")]), Err(Error::Unset("LOCALAPPDATA")));
    }

    #[test]
    fn relative_base_directory_is_refused_not_used() {
        assert_eq!(
            default_dir(Platform::Unix, &[("XDG_DATA_HOME", "data"), ("HOME", "/home/u")]),
            Err(Error::Relative("XDG_DATA_HOME"))
        );
        assert_eq!(default_dir(Platform::Unix, &[("HOME", "home/u")]), Err(Error::Relative("HOME")));
        assert_eq!(default_dir(Platform::MacOs, &[("HOME", "Users/u")]), Err(Error::Relative("HOME")));
        assert_eq!(
            default_dir(Platform::Windows, &[("LOCALAPPDATA", r"AppData\Local")]),
            Err(Error::Relative("LOCALAPPDATA"))
        );
        // A drive without a root is relative to that drive's current directory.
        assert_eq!(
            default_dir(Platform::Windows, &[("LOCALAPPDATA", "C:AppData")]),
            Err(Error::Relative("LOCALAPPDATA"))
        );
    }

    #[test]
    fn windows_accepts_drive_and_network_paths() {
        for base in [r"D:\data", "D:/data", r"\\server\share\data"] {
            let vars: &'static [(&str, &str)] = Box::leak(Box::new([("LOCALAPPDATA", base)]));
            assert_eq!(
                default_dir(Platform::Windows, vars),
                Ok(PathBuf::from(base).join("diagwire")),
                "{base}"
            );
        }
    }

    #[test]
    fn flag_beats_the_variable_and_the_variable_beats_the_default() {
        let vars: &'static [(&str, &str)] = &[(OVERRIDE_VAR, "/from/env"), ("HOME", "/home/u")];

        let flag = resolve(Some(Path::new("/from/flag")), Platform::Unix, env(vars)).unwrap();
        assert_eq!((flag.dir.as_path(), flag.chosen_by), (Path::new("/from/flag"), ChosenBy::Flag));

        let var = resolve(None, Platform::Unix, env(vars)).unwrap();
        assert_eq!((var.dir.as_path(), var.chosen_by), (Path::new("/from/env"), ChosenBy::Environment));

        // An empty variable is not a choice.
        let empty = resolve(None, Platform::Unix, env(&[(OVERRIDE_VAR, ""), ("HOME", "/home/u")])).unwrap();
        assert_eq!(empty.chosen_by, ChosenBy::Default);
    }

    #[test]
    fn a_directory_the_user_names_is_taken_as_given() {
        // No app directory is appended, a relative path is allowed, and it
        // works where there is no default at all.
        let flag = resolve(Some(Path::new("my garage")), Platform::Windows, env(&[])).unwrap();
        assert_eq!(flag.dir, PathBuf::from("my garage"));
        let var = resolve(None, Platform::Unix, env(&[(OVERRIDE_VAR, "elsewhere")])).unwrap();
        assert_eq!(var.dir, PathBuf::from("elsewhere"));
    }

    #[test]
    fn the_file_sits_inside_the_directory() {
        let resolved = resolve(Some(Path::new("/g")), Platform::Unix, env(&[])).unwrap();
        assert_eq!(resolved.file(), PathBuf::from("/g").join("garage.sqlite3"));
    }
}
