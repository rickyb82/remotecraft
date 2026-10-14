//! RustDesk IDs and the links that open a connection in the RustDesk client.
//!
//! A RustDesk ID is the number the client shows in its main window (usually 9 or 10 digits,
//! displayed in groups: `123 456 789`); a server can also give custom IDs made of letters,
//! digits, `_` and `-`. Passwords are never stored here: the person reads the one-time
//! password to the technician, and a permanent password stays in the RustDesk client.

use crate::{Error, Result};

/// Shortest and longest ID accepted.
pub const MIN_LEN: usize = 6;
pub const MAX_LEN: usize = 16;

/// The canonical form of a RustDesk ID: spaces dropped (and dashes too when the rest is
/// digits, `123-456-789`). Empty input gives an empty ID (a PC without RustDesk).
pub fn normalize(text: &str) -> Result<String> {
    let t: String = crate::clip(text, 200).chars().filter(|c| !c.is_whitespace()).collect();
    if t.is_empty() {
        return Ok(String::new());
    }
    let digits: String = t.chars().filter(|c| *c != '-').collect();
    let id = if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) { digits } else { t };
    let ok_chars = id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if !ok_chars || id.len() < MIN_LEN || id.len() > MAX_LEN {
        return Err(Error::Invalid(format!(
            "`{}` is not a RustDesk ID: {MIN_LEN}-{MAX_LEN} digits or letters, as shown in the RustDesk window",
            crate::clip(text, 40)
        )));
    }
    Ok(id)
}

/// The ID as RustDesk shows it: numeric IDs in groups of three (`123 456 789`).
pub fn display(id: &str) -> String {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
        return id.to_string();
    }
    let chars: Vec<char> = id.chars().collect();
    chars.chunks(3).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join(" ")
}

/// The link that opens a connection in the installed RustDesk client.
pub fn connect_uri(id: &str) -> String {
    format!("rustdesk://connection/new/{id}")
}

/// The command line that opens the same connection (`rustdesk --connect ID`).
pub fn connect_command(id: &str) -> String {
    format!("rustdesk --connect {id}")
}

/// The ports of a RustDesk Server OSS: what each one is for, and whether it is TCP.
pub const PORTS: &[(u16, &str)] =
    &[(21115, "hbbs: NAT type test (TCP)"), (21116, "hbbs: ID registration and connection setup (TCP and UDP)"), (21117, "hbbr: relay (TCP)")];

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                let idx = usize::try_from((n >> (18 - 6 * i)) & 63).unwrap_or(0);
                out.push(char::from(ALPHABET.get(idx).copied().unwrap_or(b'A')));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The text RustDesk's *Settings › Network › Import server config* accepts (what *Export
/// server config* produces): the JSON `{host, relay, api, key}` in base64, reversed.
pub fn config_string(host: &str, relay: &str, api: &str, key: &str) -> String {
    let json = serde_json::json!({"host": host, "relay": relay, "api": api, "key": key}).to_string();
    base64(json.as_bytes()).chars().rev().collect()
}

/// The name to give the Windows installer so it configures itself on install
/// (`rustdesk-host=HOST,key=KEY,.exe`), when the key can be part of a file name.
pub fn installer_name(host: &str, key: &str) -> Option<String> {
    let ok = |t: &str| !t.is_empty() && !t.chars().any(|c| matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | ',') || c.is_control());
    (ok(host) && ok(key)).then(|| format!("rustdesk-host={host},key={key},.exe"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes() {
        assert_eq!(normalize("123 456 789").unwrap(), "123456789");
        assert_eq!(normalize(" 123-456-7890 ").unwrap(), "1234567890");
        assert_eq!(normalize("ufficio-pc_2").unwrap(), "ufficio-pc_2");
        assert_eq!(normalize("").unwrap(), "");
        assert_eq!(normalize("   ").unwrap(), "");
    }

    #[test]
    fn rejects_hostile_ids() {
        for bad in ["12345", "12345678901234567", "123;rm -rf", "abc/def12", "pc€€€€€€", "\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}", &"9".repeat(100_000)]
        {
            assert!(normalize(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn displays_and_links() {
        assert_eq!(display("123456789"), "123 456 789");
        assert_eq!(display("1234567890"), "123 456 789 0");
        assert_eq!(display("pc-ufficio"), "pc-ufficio");
        assert_eq!(connect_uri("123456789"), "rustdesk://connection/new/123456789");
    }

    #[test]
    fn server_config_for_the_clients() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"\"}"), "In0=");
        let s = config_string("rd.example.it", "rd.example.it", "", "K3y+abc=");
        // Reversed base64 of a JSON object: `{"` is `eyJ`.
        let back: String = s.chars().rev().collect();
        assert!(back.starts_with("eyJ"), "{s}");
        assert_eq!(back.len() % 4, 0);
        assert_eq!(installer_name("rd.example.it", "K3y+abc=").unwrap(), "rustdesk-host=rd.example.it,key=K3y+abc=,.exe");
        assert!(installer_name("rd.example.it", "a/b").is_none());
        assert!(installer_name("", "k").is_none());
    }
}
