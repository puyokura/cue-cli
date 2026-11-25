// SPDX-License-Identifier: AGPL-3.0-or-later
//! Argument parsing for the `cue` CLI.
//!
//! Hand-written on purpose: the grammar is a flat
//! `command [subcommand] [positional...] [--flag [value]]` shape and the
//! dependency list stays small.
//!
//! Encoding note: values arrive through argv, which Windows passes as UTF-16
//! and Rust decodes losslessly. JSON through stdin is only used when the caller
//! writes `--json -`, because Windows PowerShell 5.1 defaults `$OutputEncoding`
//! to us-ascii and rewrites non-ASCII to `?` on the way into a native process.

use std::collections::BTreeMap;

/// Switches that never take a value.
///
/// The parser cannot tell `--live` (a bare switch) from `--frames 40` (a flag
/// with a value) by shape alone, so this list is authoritative. Without it,
/// `cue doctor --live --json` would consume `--json` as the value of `--live`.
pub const BOOLEAN_SWITCHES: &[&str] = &["confirm", "live", "verbose", "help", "recursive", "all"];

pub fn is_boolean_switch(name: &str) -> bool {
    BOOLEAN_SWITCHES.contains(&name)
}

pub struct Args {
    pub positionals: Vec<String>,
    flags: BTreeMap<String, Vec<String>>,
    switches: Vec<String>,
    /// Names given as a bare `--flag` where a value was expected.
    valueless: Vec<String>,
}

impl Args {
    pub fn parse(raw: &[String]) -> Args {
        let mut positionals = Vec::new();
        let mut flags: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut switches = Vec::new();
        let mut valueless: Vec<String> = Vec::new();
        let mut index = 0usize;

        while index < raw.len() {
            let token = &raw[index];
            if let Some(rest) = token.strip_prefix("--") {
                if rest.is_empty() {
                    // A bare `--` means "end of flags"; nothing here uses that
                    // convention, so accepting it silently would hide a mistake.
                    switches.push("--".to_string());
                    index += 1;
                    continue;
                }
                // `--flag=value` carries the value in the same token, so it
                // survives a value that starts with a dash:
                //   --text=-- leading dashes
                if let Some((name, value)) = rest.split_once('=') {
                    flags
                        .entry(name.to_string())
                        .or_default()
                        .push(value.to_string());
                    index += 1;
                    continue;
                }
                // A value is taken unless the next token is itself a flag.
                let has_value = index + 1 < raw.len() && !raw[index + 1].starts_with("--");
                if is_boolean_switch(rest) {
                    switches.push(rest.to_string());
                    index += 1;
                } else if has_value {
                    flags
                        .entry(rest.to_string())
                        .or_default()
                        .push(raw[index + 1].clone());
                    index += 2;
                } else {
                    // A known flag with no value. Recorded as well as treated as
                    // a switch, so the caller can reject it for a non-boolean.
                    switches.push(rest.to_string());
                    valueless.push(rest.to_string());
                    index += 1;
                }
            } else {
                positionals.push(token.clone());
                index += 1;
            }
        }

        Self {
            positionals,
            flags,
            switches,
            valueless,
        }
    }

    /// Every flag and switch the caller supplied, for validation.
    pub fn supplied(&self) -> Vec<String> {
        let mut names: Vec<String> = self.flags.keys().cloned().collect();
        names.extend(self.switches.iter().cloned());
        names.sort();
        names.dedup();
        names
    }

    /// Names that were supplied as a bare token (no value, not a switch).
    ///
    /// Kept separately from `supplied` so a caller can tell "flag with no value"
    /// from "switch". They are different mistakes: a valueless `--limit` should
    /// be an error, a valueless `--live` is correct.
    pub fn without_value(&self) -> Vec<String> {
        self.valueless.clone()
    }

    pub fn has(&self, name: &str) -> bool {
        self.switches.iter().any(|flag| flag == name) || self.flags.contains_key(name)
    }

    pub fn flag(&self, name: &str) -> Option<&str> {
        self.flags
            .get(name)
            .and_then(|values| values.last())
            .map(String::as_str)
    }

    pub fn flag_all(&self, name: &str) -> Vec<String> {
        self.flags.get(name).cloned().unwrap_or_default()
    }

    pub fn positional(&self, index: usize) -> Option<&str> {
        self.positionals.get(index).map(String::as_str)
    }
}

/// Interpret a `--set` value: bare `true`/`false`/`null`, integers, and
/// comma-separated lists become typed JSON; everything else stays a string.
pub fn parse_scalar(raw: &str) -> serde_json::Value {
    use serde_json::{json, Value};
    match raw {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        "null" => return Value::Null,
        _ => {}
    }
    if let Ok(number) = raw.parse::<i64>() {
        return json!(number);
    }
    if raw.contains(',') {
        return Value::Array(raw.split(',').map(|part| json!(part.trim())).collect());
    }
    json!(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boolean_switches_do_not_swallow_the_next_token() {
        let args = Args::parse(&[
            "doctor".into(),
            "--live".into(),
            "--json".into(),
            "x".into(),
        ]);
        assert!(args.has("live"));
        assert_eq!(args.flag("json"), Some("x"));
    }

    #[test]
    fn a_trailing_switch_is_a_switch() {
        let args = Args::parse(&["whoami".into(), "--verbose".into()]);
        assert!(args.has("verbose"));
    }

    #[test]
    fn a_valueless_flag_is_recorded_as_valueless() {
        // `is_boolean_switch` only knows a fixed list, so a non-boolean flag with
        // no value would otherwise pass as a switch and silently use its default.
        let args = Args::parse(&["--limit".into()]);
        assert_eq!(args.without_value(), vec!["limit".to_string()]);
        assert!(args.flag("limit").is_none());
        let ok = Args::parse(&["--live".into()]);
        assert!(ok.without_value().is_empty());
    }

    #[test]
    fn a_valued_flag_is_not_valueless() {
        let args = Args::parse(&["--limit".into(), "5".into()]);
        assert!(args.without_value().is_empty());
        assert_eq!(args.flag("limit"), Some("5"));
    }

    #[test]
    fn supplied_lists_flags_and_switches() {
        let args = Args::parse(&[
            "rpc".into(),
            "m".into(),
            "--json-file".into(),
            "a.json".into(),
            "--confirm".into(),
        ]);
        let mut names = args.supplied();
        names.sort();
        assert_eq!(names, vec!["confirm", "json-file"]);
    }

    #[test]
    fn positionals_keep_their_order() {
        // The caller strips the command name first, so `abc def` are the
        // positionals of `cue session abc def`.
        let args = Args::parse(&["abc".into(), "def".into()]);
        assert_eq!(args.positional(0), Some("abc"));
        assert_eq!(args.positional(1), Some("def"));
    }

    #[test]
    fn dash_is_a_value_not_a_flag() {
        // `--json -` must read stdin, so a lone `-` has to reach the flag value.
        let args = Args::parse(&["rpc".into(), "m".into(), "--json".into(), "-".into()]);
        assert_eq!(args.flag("json"), Some("-"));
    }

    #[test]
    fn equals_form_survives_a_dash_value() {
        // A prompt that starts with dashes is legal text, not a flag.
        let args = Args::parse(&["--text=-- leading dashes".into()]);
        assert_eq!(args.flag("text"), Some("-- leading dashes"));
    }

    #[test]
    fn equals_form_and_plain_form_agree() {
        let a = Args::parse(&["--limit=5".into()]);
        let b = Args::parse(&["--limit".into(), "5".into()]);
        assert_eq!(a.flag("limit"), b.flag("limit"));
    }

    #[test]
    fn scalars_are_typed() {
        assert_eq!(parse_scalar("true"), serde_json::json!(true));
        assert_eq!(parse_scalar("12"), serde_json::json!(12));
        assert_eq!(parse_scalar("a,b"), serde_json::json!(["a", "b"]));
        assert_eq!(parse_scalar("text"), serde_json::json!("text"));
    }
}
