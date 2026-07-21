//! Policy and value marshalling for the raw XML-RPC console (D15).
//!
//! The console lets a power user send any method the daemon exposes, so this
//! module is the safety boundary. Two independent gates apply:
//!
//!   * **Always blocked** — the `execute.*` and `method.insert` / `method.erase`
//!     / `method.set_key` families let a caller run arbitrary shell commands or
//!     redefine daemon methods. They are refused unconditionally, and (because a
//!     `*.multicall` can smuggle them as nested method names) we also scan the
//!     arguments of any multicall for those names.
//!   * **Mutation gate** — anything that looks like it changes daemon state is
//!     refused unless the caller armed "Allow mutations" for the session. The
//!     classifier is a naming heuristic (rtorrent has no formal read/write
//!     split); it errs toward *not* blocking plain getters so the read-only tool
//!     stays useful, while the hard block above still catches the truly
//!     dangerous methods even if the heuristic misjudges them.
//!
//! It also converts between the JSON the frontend sends/receives and the
//! [`Value`] the transport speaks.

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};

use crate::rtorrent::xmlrpc::Value;

/// Method-name prefixes that are never allowed from the console (shell exec /
/// method redefinition), regardless of the mutation toggle.
const BLOCKED_PREFIXES: &[&str] = &[
    "execute",
    "method.insert",
    "method.erase",
    "method.set_key",
];

/// Dot-delimited segments that mark a method as mutating (state-changing). Kept
/// to exact-segment matches (plus the `set` / `set_*` special-case) so getters
/// like `network.open_files` or `d.is_open` are not swept up.
const MUTATING_SEGMENTS: &[&str] = &[
    "start", "stop", "close", "open", "erase", "pause", "resume", "save", "load", "insert",
    "create", "remove", "clear", "reset", "disable", "enable", "announce", "connect",
    "disconnect", "ban", "unban", "add", "delete", "kill", "choke", "unchoke", "import", "toggle",
    "shutdown", "send", "schedule", "redirect", "unload",
];

/// Does this method name run shell commands or redefine daemon methods?
fn is_hard_blocked_name(method_lower: &str) -> bool {
    BLOCKED_PREFIXES
        .iter()
        .any(|p| method_lower.starts_with(p))
}

/// Is this one of rtorrent's `*.multicall` batch methods?
fn is_multicall(method_lower: &str) -> bool {
    method_lower.contains("multicall")
}

/// Does any string anywhere in the argument tree name a blocked method? Used to
/// stop a multicall from smuggling `execute.*` past the top-level name check.
fn args_reference_blocked(params: &[Value]) -> bool {
    params.iter().any(value_references_blocked)
}

fn value_references_blocked(v: &Value) -> bool {
    match v {
        Value::Str(s) => {
            let s = s.to_ascii_lowercase();
            BLOCKED_PREFIXES.iter().any(|b| s.contains(b))
        }
        Value::Array(a) => a.iter().any(value_references_blocked),
        Value::Struct(m) => m.iter().any(|(_, val)| value_references_blocked(val)),
        _ => false,
    }
}

/// Heuristic: would this method change daemon state? See the module docs for the
/// intentional read-leaning bias.
pub fn is_mutating(method: &str) -> bool {
    let m = method.to_ascii_lowercase();
    if m.ends_with(".set") || m.contains(".set_") {
        return true;
    }
    m.split('.').any(|seg| {
        seg == "set" || seg.starts_with("set_") || MUTATING_SEGMENTS.contains(&seg)
    })
}

/// Vet a method + args against the console policy. `Ok(())` means "send it";
/// `Err` carries a message shown verbatim to the user.
pub fn check_policy(method: &str, params: &[Value], allow_mutations: bool) -> Result<(), String> {
    let trimmed = method.trim();
    if trimmed.is_empty() {
        return Err("Enter a method name.".into());
    }
    let m = trimmed.to_ascii_lowercase();

    if is_hard_blocked_name(&m) || (is_multicall(&m) && args_reference_blocked(params)) {
        return Err(format!(
            "Blocked: '{trimmed}' can run arbitrary shell commands or redefine daemon methods, \
             and is never allowed from the console."
        ));
    }

    if !allow_mutations && (is_mutating(&m) || is_multicall(&m)) {
        let what = if is_multicall(&m) {
            "batches calls that may change daemon state"
        } else {
            "looks like it would change daemon state"
        };
        return Err(format!(
            "'{trimmed}' {what}. Enable \u{201c}Allow mutations\u{201d} to run it."
        ));
    }

    Ok(())
}

/// Parse the console's argument box (JSON array text) into transport values.
/// Empty/whitespace yields no arguments.
pub fn parse_args(text: &str) -> Result<Vec<Value>, String> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(vec![]);
    }
    let json: serde_json::Value =
        serde_json::from_str(t).map_err(|e| format!("Arguments must be a JSON array — {e}"))?;
    match json {
        serde_json::Value::Array(items) => items.iter().map(json_to_value).collect(),
        _ => Err("Arguments must be a JSON array, e.g. [\"<hash>\", \"\"].".into()),
    }
}

fn json_to_value(j: &serde_json::Value) -> Result<Value, String> {
    Ok(match j {
        // XML-RPC has no null; an explicit null becomes an empty string.
        serde_json::Value::Null => Value::Str(String::new()),
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(i) => Value::Int(i),
            None => Value::Double(n.as_f64().unwrap_or(0.0)),
        },
        serde_json::Value::String(s) => Value::Str(s.clone()),
        serde_json::Value::Array(a) => {
            Value::Array(a.iter().map(json_to_value).collect::<Result<_, _>>()?)
        }
        serde_json::Value::Object(o) => Value::Struct(
            o.iter()
                .map(|(k, v)| Ok((k.clone(), json_to_value(v)?)))
                .collect::<Result<Vec<_>, String>>()?,
        ),
    })
}

/// Convert a decoded XML-RPC value into JSON for pretty-printing in the console.
/// Raw byte buffers (e.g. `d.chunks_seen`) are rendered as `base64:<…>`.
pub fn value_to_json(v: &Value) -> serde_json::Value {
    use serde_json::Value as J;
    match v {
        Value::Int(i) => J::Number((*i).into()),
        Value::Bool(b) => J::Bool(*b),
        Value::Str(s) => J::String(s.clone()),
        Value::Double(d) => serde_json::Number::from_f64(*d).map(J::Number).unwrap_or(J::Null),
        Value::Bytes(b) => J::String(format!("base64:{}", B64.encode(b))),
        Value::Array(a) => J::Array(a.iter().map(value_to_json).collect()),
        Value::Struct(m) => J::Object(
            m.iter()
                .map(|(k, val)| (k.clone(), value_to_json(val)))
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> Value {
        Value::Str(v.to_string())
    }

    #[test]
    fn getters_are_not_mutating() {
        for m in [
            "d.name",
            "d.size_bytes",
            "d.complete",
            "throttle.global_up.rate",
            "network.open_files",   // 'open_files' must not read as 'open'
            "network.open_sockets",
            "d.is_open",
            "session.path",
            "system.listMethods",
            "system.client_version",
        ] {
            assert!(!is_mutating(m), "{m} should be read-only");
        }
    }

    #[test]
    fn setters_and_verbs_are_mutating() {
        for m in [
            "d.start",
            "d.stop",
            "d.erase",
            "load.normal",
            "session.save",
            "network.listen.open",
            "throttle.global_up.max_rate.set",
            "d.priority.set",
            "d.set_priority", // older set_* form
            "group.insert",
            "system.shutdown.normal",
        ] {
            assert!(is_mutating(m), "{m} should be mutating");
        }
    }

    #[test]
    fn execute_and_method_insert_are_hard_blocked_even_when_armed() {
        for m in [
            "execute",
            "execute.throw",
            "execute.nothrow",
            "execute2",
            "method.insert",
            "method.erase",
            "method.set_key",
        ] {
            assert!(
                check_policy(m, &[], true).is_err(),
                "{m} must be blocked even with mutations armed"
            );
        }
    }

    #[test]
    fn multicall_cannot_smuggle_a_blocked_method() {
        // system.multicall carrying an execute.* call, mutations armed.
        let params = vec![Value::Array(vec![Value::Struct(vec![
            ("methodName".into(), s("execute.throw")),
            ("params".into(), Value::Array(vec![s(""), s("rm -rf /")])),
        ])])];
        assert!(check_policy("system.multicall", &params, true).is_err());
    }

    #[test]
    fn mutation_gate_blocks_only_when_disarmed() {
        assert!(check_policy("d.start", &[], false).is_err());
        assert!(check_policy("d.start", &[], true).is_ok());
        // Getters run without arming.
        assert!(check_policy("d.name", &[s("HASH")], false).is_ok());
        // A read-only-looking multicall still needs arming (contents are opaque).
        assert!(check_policy("d.multicall2", &[s(""), s("main")], false).is_err());
        assert!(check_policy("d.multicall2", &[s(""), s("main")], true).is_ok());
    }

    #[test]
    fn empty_method_is_rejected() {
        assert!(check_policy("  ", &[], true).is_err());
    }

    #[test]
    fn parse_args_handles_empty_types_and_shape() {
        assert_eq!(parse_args("").unwrap(), Vec::<Value>::new());
        assert_eq!(parse_args("   ").unwrap(), Vec::<Value>::new());
        let got = parse_args(r#"["abc", 42, true, 1.5, ["x"], {"k": "v"}]"#).unwrap();
        assert_eq!(
            got,
            vec![
                s("abc"),
                Value::Int(42),
                Value::Bool(true),
                Value::Double(1.5),
                Value::Array(vec![s("x")]),
                Value::Struct(vec![("k".into(), s("v"))]),
            ]
        );
        // Non-array and malformed JSON are errors.
        assert!(parse_args(r#""bare string""#).is_err());
        assert!(parse_args("[unclosed").is_err());
    }

    #[test]
    fn value_to_json_covers_every_variant() {
        assert_eq!(value_to_json(&Value::Int(7)), serde_json::json!(7));
        assert_eq!(value_to_json(&Value::Bool(true)), serde_json::json!(true));
        assert_eq!(value_to_json(&s("hi")), serde_json::json!("hi"));
        assert_eq!(
            value_to_json(&Value::Bytes(vec![104, 105])),
            serde_json::json!("base64:aGk=")
        );
        assert_eq!(
            value_to_json(&Value::Array(vec![Value::Int(1), s("a")])),
            serde_json::json!([1, "a"])
        );
        assert_eq!(
            value_to_json(&Value::Struct(vec![("n".into(), Value::Int(2))])),
            serde_json::json!({ "n": 2 })
        );
    }
}
