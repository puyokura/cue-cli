// SPDX-License-Identifier: AGPL-3.0-or-later
//! Health checks that answer "why is this not working" without guessing.
//!
//! Every check is independent and reports its own result, so a failure names the
//! layer that broke instead of collapsing into one error. Nothing here changes
//! state: the live checks are read-only RPCs, and the local checks only look at
//! the state directory.
//!
//! Identifiers are never printed. A check reports counts and booleans, because
//! the point of `doctor` is to find the broken layer, not to echo the account.

use serde_json::{json, Value};

#[derive(Clone, Copy)]
pub enum Status {
    Ok,
    Warn,
    Fail,
    Skip,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Warn => "warn",
            Status::Fail => "fail",
            Status::Skip => "skip",
        }
    }

    /// Whether this status makes the overall result a failure.
    fn is_failure(self) -> bool {
        matches!(self, Status::Fail)
    }
}

pub struct Check {
    pub name: &'static str,
    pub status: Status,
    pub detail: String,
}

impl Check {
    fn new(name: &'static str, status: Status, detail: impl Into<String>) -> Self {
        Self {
            name,
            status,
            detail: detail.into(),
        }
    }
}

/// Everything the doctor looked at.
pub struct Report {
    pub checks: Vec<Check>,
}

impl Report {
    pub fn failed(&self) -> bool {
        self.checks.iter().any(|check| check.status.is_failure())
    }

    pub fn to_json(&self, live: bool) -> Value {
        let checks: Vec<Value> = self
            .checks
            .iter()
            .map(|check| {
                json!({
                  "name": check.name,
                  "status": check.status.label(),
                  "detail": check.detail,
                })
            })
            .collect();
        let mut counts = json!({"ok": 0, "warn": 0, "fail": 0, "skip": 0});
        for check in &self.checks {
            let key = check.status.label();
            let next = counts[key].as_u64().unwrap_or(0) + 1;
            counts[key] = json!(next);
        }
        json!({
          "live": live,
          "failed": self.failed(),
          "counts": counts,
          "checks": checks,
        })
    }

    /// Human-readable rendering: one line per check, then a verdict.
    pub fn to_text(&self, live: bool) -> String {
        let mut out = String::new();
        for check in &self.checks {
            out.push_str(&format!(
                "  {:<6} {:<12} {}\n",
                check.status.label(),
                check.name,
                check.detail
            ));
        }
        let failed = self.checks.iter().filter(|c| c.status.is_failure()).count();
        let verdict = if failed == 0 {
            "all checks passed".to_string()
        } else {
            format!("{failed} check(s) failed")
        };
        out.push_str(&format!(
            "\n{verdict} ({} checks, live={live})\n",
            self.checks.len()
        ));
        out
    }
}

fn describe_state(paths: &crate::state::StatePaths) -> Check {
    let token = &paths.token;
    // Report the same number `auth` reports: the trimmed token length. A raw file
    // length would disagree by the trailing newline, which reads as a bug.
    let bytes = paths.read_token().map(|s| s.len()).unwrap_or(0);
    let token_exists = bytes > 0;
    let mode = {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::metadata(token)
                .map(|m| format!("{:04o}", m.permissions().mode() & 0o777))
                .unwrap_or_else(|_| "-".to_string())
        }
        #[cfg(not(unix))]
        {
            "-".to_string()
        }
    };
    let detail = if token_exists {
        format!("token at {} ({bytes} bytes, mode {mode})", token.display())
    } else {
        format!("no token file at {}", token.display())
    };
    // A check that cannot fail is not a check. A missing state directory is a
    // warning rather than a failure: the CLI is usable once `login` runs.
    let status = if token_exists {
        Status::Ok
    } else {
        Status::Warn
    };
    Check::new("state", status, detail)
}

fn describe_auth(has_token: bool, token_bytes: usize) -> Check {
    if has_token {
        Check::new(
            "auth",
            Status::Ok,
            format!("token stored ({token_bytes} bytes); value not shown"),
        )
    } else {
        Check::new("auth", Status::Fail, "no token; run `cue login`")
    }
}

/// Run the local checks, then the live ones when `live` is set.
///
/// `live` performs two read-only RPCs: one public (no token) and, when a token
/// exists, one authenticated. Both are GET-like; nothing here mutates the
/// account.
pub fn run(live: bool) -> Report {
    let mut report = Report { checks: Vec::new() };

    // Resolve paths without creating anything: `doctor` must be able to report a
    // missing state directory rather than create it as a side effect. It also has
    // to honour a file-shaped state path, which `state_dir()` would flatten into
    // its parent directory and then look for a `token` that is not there.
    let paths = match crate::state_paths() {
        Ok(paths) => Some(paths),
        Err(err) => {
            report.checks.push(Check::new("state", Status::Fail, err));
            None
        }
    };

    let (has_token, token_bytes) = match &paths {
        Some(paths) => {
            let bytes = paths.read_token().map(|s| s.len()).unwrap_or(0);
            report.checks.push(describe_state(paths));
            (bytes > 0, bytes)
        }
        None => (false, 0),
    };
    report.checks.push(describe_auth(has_token, token_bytes));

    // Resolve the origin the same way every command does: a `login --base` value
    // stored beside the token wins, so the health check probes the host the
    // commands will actually use.
    let base = crate::commands::resolve_origin();

    // A base that is not an absolute http(s) URL cannot be reached at all, and
    // saying "ok" would send the caller looking at the token instead.
    let base_check = if base.starts_with("https://") || base.starts_with("http://") {
        Check::new("base", Status::Ok, base.clone())
    } else {
        Check::new(
            "base",
            Status::Fail,
            format!("{base} is not an http(s) URL"),
        )
    };
    report.checks.push(base_check);

    if !live {
        report.checks.push(Check::new(
            "gateway",
            Status::Skip,
            "not checked (pass --live)",
        ));
        report.checks.push(Check::new(
            "identity",
            Status::Skip,
            "not checked (pass --live)",
        ));
        report.checks.push(Check::new(
            "region",
            Status::Skip,
            "not checked (pass --live)",
        ));
        return report;
    }

    // Public RPC: proves the transport and the product header reach the service.
    report.checks.push(
        match crate::client::call_json(
            &base,
            "/user.v1.UserPublicService/GetGlobalSettings",
            None,
            &json!({}),
        ) {
            Ok(value) => {
                let keys = value.as_object().map(|m| m.len()).unwrap_or(0);
                Check::new(
                    "gateway",
                    Status::Ok,
                    format!("public RPC ok ({keys} fields)"),
                )
            }
            Err(err) => Check::new(
                "gateway",
                Status::Fail,
                format!("{err}  (nothing else can work)"),
            ),
        },
    );

    // Authenticated RPC: proves the token is still accepted by the service.
    if has_token {
        let token = crate::client::stored_token();
        report.checks.push(
            match crate::client::call_json(
                &base,
                "/user.v1.UserService/UserInfo",
                token.as_deref(),
                &json!({}),
            ) {
                Ok(value) => {
                    let fields = value.as_object().map(|m| m.len()).unwrap_or(0);
                    Check::new(
                        "identity",
                        Status::Ok,
                        format!("token accepted ({fields} fields); values not shown"),
                    )
                }
                Err(err) => {
                    // A 5xx means the service is unwell, not that this token is stale;
                    // so does a transport failure (no HTTP status at all). Only a 4xx
                    // says "re-login", and telling someone to re-login during an outage
                    // sends them down the wrong path.
                    if err.starts_with("HTTP 4") {
                        Check::new("identity", Status::Fail, format!("{err}; run `cue login`"))
                    } else {
                        Check::new(
                            "identity",
                            Status::Warn,
                            format!("service unavailable: {err}"),
                        )
                    }
                }
            },
        );
    } else {
        report
            .checks
            .push(Check::new("identity", Status::Skip, "no token to verify"));
    }

    // Region: a second public RPC, so a gateway failure is not confused with one
    // endpoint being down.
    report.checks.push(
        match crate::client::call_json(
            &base,
            "/user.v1.UserPublicService/CheckRegion",
            None,
            &json!({}),
        ) {
            Ok(value) => {
                let iso = value.get("iso").and_then(Value::as_str).unwrap_or("-");
                let code = value.get("code").and_then(Value::as_str).unwrap_or("-");
                Check::new("region", Status::Ok, format!("{iso} {code}"))
            }
            Err(err) => Check::new("region", Status::Fail, err),
        },
    );

    report
}

/// Status: what the CLI thinks its own state is, without changing anything.
///
/// Reported as JSON so a caller can act on it. `signed_in` means a token is
/// stored, not that the service still accepts it; when a token exists the
/// result also carries `identity_check` for the live answer.
pub fn status() -> Value {
    let (state_path, state_error) = match crate::state_paths().map(|p| p.dir) {
        Ok(dir) => (dir.display().to_string(), None),
        Err(err) => ("-".to_string(), Some(err)),
    };
    let token = crate::client::stored_token();
    let signed_in = token.is_some();
    let base = crate::commands::resolve_origin();

    let identity_check = match (&token, state_error.is_none()) {
        (Some(t), true) => match crate::client::call_json(
            &base,
            "/user.v1.UserService/UserInfo",
            Some(t.as_str()),
            &json!({}),
        ) {
            Ok(value) => {
                let fields = value.as_object().map(|m| m.len()).unwrap_or(0);
                json!({"ok": true, "fields": fields})
            }
            Err(err) => json!({"ok": false, "error": err}),
        },
        _ => json!({"ok": false, "error": "no token"}),
    };

    json!({
      "state_path": state_path,
      "signed_in": signed_in,
      "base": base,
      "identity_check": identity_check,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report_of(checks: Vec<Check>) -> Report {
        Report { checks }
    }

    #[test]
    fn a_failing_check_fails_the_report() {
        let report = report_of(vec![
            Check::new("state", Status::Ok, "fine"),
            Check::new("auth", Status::Fail, "not signed in"),
        ]);
        assert!(report.failed());
    }

    #[test]
    fn skips_and_warnings_do_not_fail_the_report() {
        let report = report_of(vec![
            Check::new("state", Status::Ok, "fine"),
            Check::new("gateway", Status::Skip, "not checked"),
            Check::new("identity", Status::Warn, "unverified"),
        ]);
        assert!(!report.failed());
    }

    #[test]
    fn json_keeps_each_status_label() {
        let report = report_of(vec![
            Check::new("state", Status::Ok, "a"),
            Check::new("gateway", Status::Fail, "b"),
            Check::new("identity", Status::Skip, "c"),
            Check::new("auth", Status::Warn, "d"),
        ]);
        let json = report.to_json(true);
        assert_eq!(json["checks"].as_array().unwrap().len(), 4);
        assert_eq!(json["counts"]["ok"], 1);
        assert_eq!(json["counts"]["fail"], 1);
        assert_eq!(json["counts"]["skip"], 1);
        assert_eq!(json["counts"]["warn"], 1);
        assert_eq!(json["live"], true);
        assert_eq!(json["failed"], true);
    }

    #[test]
    fn text_rendering_reports_the_verdict() {
        let report = report_of(vec![Check::new("state", Status::Ok, "fine")]);
        let text = report.to_text(false);
        assert!(text.contains("all checks passed"));
        assert!(text.contains("live=false"));
    }
}
