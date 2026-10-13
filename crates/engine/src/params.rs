//! Reading command parameters. Parameters are hostile: wrong types are errors, text is
//! cleaned and clipped, numbers are checked.

use remotecraft_core::{Error, Result, clean, limits, line};
use serde_json::Value;

fn wrong(key: &str, what: &str) -> Error {
    Error::Invalid(format!("`{key}` must be {what}"))
}

/// A one-line text parameter (numbers are accepted as text); `None` when absent or null.
pub fn text(p: &Value, key: &str) -> Result<Option<String>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(line(s))),
        Some(Value::Number(n)) => Ok(Some(n.to_string())),
        Some(_) => Err(wrong(key, "a string")),
    }
}

/// A multi-line text parameter, clipped to `max` bytes.
pub fn notes(p: &Value, key: &str, max: usize) -> Result<Option<String>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(clean(s, max))),
        Some(_) => Err(wrong(key, "a string")),
    }
}

/// A required one-line text, not empty.
pub fn required(p: &Value, key: &str) -> Result<String> {
    text(p, key)?.filter(|s| !s.is_empty()).ok_or_else(|| Error::Invalid(format!("`{key}` is required")))
}

pub fn boolean(p: &Value, key: &str) -> Result<Option<bool>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(Value::String(s)) => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "yes" | "si" | "sì" | "1" | "on" => Ok(Some(true)),
            "false" | "no" | "0" | "off" => Ok(Some(false)),
            _ => Err(wrong(key, "true or false")),
        },
        Some(Value::Number(n)) => Ok(Some(n.as_u64().unwrap_or(0) != 0)),
        Some(_) => Err(wrong(key, "true or false")),
    }
}

/// A non-negative integer (numbers or digits in a string, `#12` too).
pub fn number(p: &Value, key: &str) -> Result<Option<u64>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => n.as_u64().map(Some).ok_or_else(|| wrong(key, "a whole number ≥ 0")),
        Some(Value::String(s)) if s.trim().is_empty() => Ok(None),
        Some(Value::String(s)) => s.trim().trim_start_matches('#').parse().map(Some).map_err(|_| wrong(key, "a whole number ≥ 0")),
        Some(_) => Err(wrong(key, "a whole number ≥ 0")),
    }
}

/// A non-negative integer up to `max`.
pub fn bounded(p: &Value, key: &str, max: u64) -> Result<Option<u32>> {
    match number(p, key)? {
        None => Ok(None),
        Some(n) if n <= max => u32::try_from(n).map(Some).map_err(|_| wrong(key, "smaller")),
        Some(_) => Err(Error::Invalid(format!("`{key}` must be at most {max}"))),
    }
}

/// A list of short texts: a JSON array of strings, or one string separated by commas.
pub fn list(p: &Value, key: &str, max_items: usize) -> Result<Option<Vec<String>>> {
    let items: Vec<String> = match p.get(key) {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::String(s)) => s.split(',').map(line).collect(),
        Some(Value::Array(a)) => {
            let mut out = Vec::new();
            for v in a.iter().take(max_items.saturating_mul(2)) {
                match v {
                    Value::String(s) => out.push(line(s)),
                    _ => return Err(wrong(key, "a list of strings")),
                }
            }
            out
        }
        Some(_) => return Err(wrong(key, "a list of strings")),
    };
    let mut out: Vec<String> = Vec::new();
    for i in items.into_iter().filter(|s| !s.is_empty()) {
        if !out.contains(&i) {
            out.push(i);
        }
    }
    if out.len() > max_items {
        return Err(Error::Invalid(format!("`{key}` has more than {max_items} items")));
    }
    Ok(Some(out))
}

/// `limit` (default `default`, at most [`limits::MAX_LIST`]) and `offset`.
pub fn page(p: &Value, default: usize) -> Result<(usize, usize)> {
    let limit = number(p, "limit")?.map_or(default, |n| usize::try_from(n).unwrap_or(limits::MAX_LIST)).clamp(1, limits::MAX_LIST);
    let offset = number(p, "offset")?.map_or(0, |n| usize::try_from(n).unwrap_or(usize::MAX));
    Ok((limit, offset))
}

/// The first of several keys that is present (aliases: `customer`, `customer_id`, `id`).
pub fn first<'a>(p: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|k| p.get(*k).filter(|v| !v.is_null() && v.as_str().is_none_or(|s| !s.trim().is_empty())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn types_are_checked() {
        let p = json!({"s": "  a\nb ", "n": 5, "neg": -1, "b": "sì", "arr": ["x", "x", " y "], "csv": "a, b,,c", "bad": [1], "obj": {}});
        assert_eq!(text(&p, "s").unwrap().unwrap(), "a b");
        assert_eq!(text(&p, "n").unwrap().unwrap(), "5");
        assert!(text(&p, "obj").is_err());
        assert_eq!(number(&p, "n").unwrap(), Some(5));
        assert!(number(&p, "neg").is_err());
        assert!(number(&p, "s").is_err());
        assert_eq!(boolean(&p, "b").unwrap(), Some(true));
        assert_eq!(list(&p, "arr", 10).unwrap().unwrap(), vec!["x", "y"]);
        assert_eq!(list(&p, "csv", 10).unwrap().unwrap(), vec!["a", "b", "c"]);
        assert!(list(&p, "bad", 10).is_err());
        assert!(list(&p, "csv", 2).is_err());
        assert!(bounded(&json!({"m": 99_999_999_999u64}), "m", 100).is_err());
        assert_eq!(page(&json!({"limit": 0}), 50).unwrap(), (1, 0));
        assert_eq!(page(&json!({"limit": u64::MAX}), 50).unwrap().0, limits::MAX_LIST);
        assert!(required(&json!({"x": "  "}), "x").is_err());
        assert_eq!(first(&json!({"a": "", "b": 3}), &["a", "b"]), Some(&json!(3)));
    }
}
