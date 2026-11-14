use std::fmt::Display;
use std::path::{Path, PathBuf};

pub const REQUIRE_VAR: &str = "CITRINE_REQUIRE_ROMS";

pub fn required() -> bool {
    std::env::var_os(REQUIRE_VAR).is_some_and(|value| !value.is_empty() && value != "0")
}

pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn roms_dir() -> PathBuf {
    workspace_root().join(".roms")
}

pub fn missing(test: &str, what: impl Display) {
    missing_when(required(), test, what);
}

pub fn read(test: &str, path: impl AsRef<Path>) -> Option<Vec<u8>> {
    read_when(required(), test, path.as_ref())
}

pub fn env_path(test: &str, var: &str) -> Option<PathBuf> {
    env_path_when(required(), test, var)
}

pub fn dir(test: &str, path: impl AsRef<Path>) -> Option<PathBuf> {
    dir_when(required(), test, path.as_ref())
}

pub fn found<T>(test: &str, what: impl Display, value: Option<T>) -> Option<T> {
    if value.is_none() {
        missing(test, what);
    }
    value
}

fn missing_when(required: bool, test: &str, what: impl Display) {
    assert!(
        !required,
        "{test}: missing {what}, and {REQUIRE_VAR} is set"
    );
    eprintln!("{test}: skipped, needs {what}");
}

fn read_when(required: bool, test: &str, path: &Path) -> Option<Vec<u8>> {
    match std::fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) => {
            missing_when(required, test, format_args!("{} ({error})", path.display()));
            None
        }
    }
}

fn env_path_when(required: bool, test: &str, var: &str) -> Option<PathBuf> {
    let path = std::env::var_os(var).map(PathBuf::from);
    if path.is_none() {
        missing_when(required, test, format_args!("{var}, which names its file"));
    }
    path
}

fn dir_when(required: bool, test: &str, path: &Path) -> Option<PathBuf> {
    if path.is_dir() {
        return Some(path.to_path_buf());
    }
    missing_when(
        required,
        test,
        format_args!("the folder {}", path.display()),
    );
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const ABSENT: &str = "no-such-dir/absent.gb";

    #[test]
    fn a_missing_rom_skips_when_the_run_expects_none() {
        assert_eq!(read_when(false, "skip_test", Path::new(ABSENT)), None);
        assert_eq!(dir_when(false, "skip_test", Path::new(ABSENT)), None);
        assert_eq!(
            env_path_when(false, "skip_test", "CITRINE_TEST_ROMS_UNSET_VAR"),
            None
        );
    }

    #[test]
    #[should_panic(expected = "fail_test: missing no-such-dir/absent.gb")]
    fn a_missing_rom_fails_naming_its_path_when_the_run_expects_roms() {
        read_when(true, "fail_test", Path::new(ABSENT));
    }

    #[test]
    #[should_panic(
        expected = "missing the folder no-such-dir/absent.gb, and CITRINE_REQUIRE_ROMS is set"
    )]
    fn a_missing_folder_fails_when_the_run_expects_roms() {
        dir_when(true, "fail_test", Path::new(ABSENT));
    }

    #[test]
    #[should_panic(expected = "missing CITRINE_TEST_ROMS_UNSET_VAR")]
    fn an_unset_rom_variable_fails_when_the_run_expects_roms() {
        env_path_when(true, "fail_test", "CITRINE_TEST_ROMS_UNSET_VAR");
    }

    #[test]
    fn a_present_file_reads_either_way() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        for required in [false, true] {
            assert!(read_when(required, "read_test", &path).is_some());
            assert!(dir_when(required, "read_test", path.parent().unwrap()).is_some());
        }
    }
}
