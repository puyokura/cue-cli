// SPDX-License-Identifier: AGPL-3.0-or-later
//! Where the CLI keeps its token and base URL.
//!
//! Resolution order, first non-empty wins:
//!
//! 1. `--state PATH` (handled by the caller, which sets `CUE_STATE`)
//! 2. `CUE_STATE`
//! 3. `CUE_MCP_STATE` (the sibling-CLI spelling; `muse` and `pickle` accept it)
//! 4. the OS config directory
//!
//! A value may name either a **directory** or a **file**. Both spellings work
//! and mean the same thing once resolved:
//!
//! * directory → token at `<dir>/token`, base at `<dir>/base`
//! * file       → that file is the token (<file>), base at `<file>.base`
//!
//! The file spelling exists because the sibling CLIs take a session **file**
//! path; refusing it here would make one env var mean different things across
//! the three tools.

use std::fs;
use std::path::PathBuf;

pub struct StatePaths {
    pub dir: PathBuf,
    pub token: PathBuf,
    pub base: PathBuf,
}

impl StatePaths {
    /// Resolve without creating anything, so `doctor` can report a path that
    /// does not exist yet instead of creating it as a side effect.
    pub fn resolve() -> Result<Self, String> {
        let candidate = first_env(&["CUE_STATE", "CUE_MCP_STATE"]);
        let raw = match candidate {
            Some(value) => PathBuf::from(value),
            None => os_default_dir()?,
        };
        Ok(Self::from_path(raw))
    }

    fn from_path(raw: PathBuf) -> Self {
        if raw.is_file() {
            // A bare filename has an empty parent; reporting `dir` as "" then
            // prints a blank path. Fall back to "." so the report names the
            // directory the caller is actually in.
            let dir = match raw.parent() {
                Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
                _ => PathBuf::from("."),
            };
            let base = raw.with_extension("base");
            return Self {
                dir,
                token: raw,
                base,
            };
        }
        Self {
            token: raw.join("token"),
            base: raw.join("base"),
            dir: raw,
        }
    }

    /// Create the directory and tighten its permissions.
    pub fn ensure(&self) -> Result<(), String> {
        fs::create_dir_all(&self.dir).map_err(|e| format!("create {}: {e}", self.dir.display()))?;
        crate::restrict_path(&self.dir)?;
        Ok(())
    }

    pub fn read_token(&self) -> Option<String> {
        let raw = fs::read_to_string(&self.token).ok()?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    pub fn read_base(&self) -> Option<String> {
        let raw = fs::read_to_string(&self.base).ok()?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }
}

fn first_env(names: &[&str]) -> Option<String> {
    for name in names {
        if let Ok(value) = std::env::var(name) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn os_default_dir() -> Result<PathBuf, String> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    let base = base.ok_or(
        "cannot locate a config directory (set APPDATA, XDG_CONFIG_HOME, CUE_STATE or CUE_MCP_STATE)",
    )?;
    Ok(base.join("cue-cli"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_directory_value_names_token_and_base() {
        let p = StatePaths::from_path(PathBuf::from("some/dir"));
        assert_eq!(p.token, PathBuf::from("some/dir/token"));
        assert_eq!(p.base, PathBuf::from("some/dir/base"));
    }

    #[test]
    fn a_file_value_is_the_token_itself() {
        let dir = std::env::temp_dir().join("cue-state-file-test");
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("session.json");
        fs::write(&file, "x").unwrap();
        let p = StatePaths::from_path(file.clone());
        assert_eq!(p.token, file);
        assert_eq!(p.base, file.with_extension("base"));
        assert_eq!(p.dir, dir);
        let _ = fs::remove_file(&file);
    }

    #[test]
    fn an_empty_value_is_skipped() {
        // A set-but-empty variable must not win: it would resolve to a relative
        // path and put state next to the current directory.
        std::env::set_var("CUE_STATE_TEST_EMPTY", "");
        std::env::remove_var("CUE_STATE_TEST_EMPTY");
        assert!(first_env(&["CUE_STATE_TEST_EMPTY"]).is_none());
    }
}
