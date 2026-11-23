//! Hashing helpers shared by the conformance runner and the public-fixture generator.
//!
//! The canonical model hash must match the JavaScript harness: object keys are sorted, arrays keep their order, and numbers use
//! ECMAScript's shortest round-trip formatting. Keeping the implementation here means the generator and the checker cannot drift.
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

pub fn hex_digest(digest: &[u8]) -> String {
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

pub fn sha256(bytes: &[u8]) -> String {
    hex_digest(&Sha256::digest(bytes))
}

/// Format an f64 the way ECMAScript `JSON.stringify` does: shortest
/// round-trip digits, fixed notation for 1e-6 <= |x| < 1e21, exponent
/// otherwise, and `0` for negative zero.
pub fn js_number(x: f64) -> String {
    if x == 0.0 {
        return "0".to_string();
    }
    let negative = x < 0.0;
    let abs = x.abs();
    // serde_json/ryu gives a shortest round-trip representation; reformat it
    // without changing the digits. `float_roundtrip` is enabled for parsing.
    let raw = serde_json::to_string(&abs).unwrap_or_else(|_| abs.to_string());
    let (mantissa, exp) = match raw.split_once(['e', 'E']) {
        Some((m, e)) => (m, e.parse::<i32>().unwrap_or(0)),
        None => (raw.as_str(), 0),
    };
    let mut digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let frac = mantissa
        .split_once('.')
        .map(|(_, f)| f.len() as i32)
        .unwrap_or(0);
    while digits.len() > 1 && digits.ends_with('0') {
        digits.pop();
    }
    let e10 = exp - frac;
    let n = digits.len() as i32;
    let point = n + e10;
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if (1e-6..1e21).contains(&abs) {
        if point <= 0 {
            out.push_str("0.");
            for _ in 0..(-point) {
                out.push('0');
            }
            out.push_str(&digits);
        } else if point >= n {
            out.push_str(&digits);
            for _ in 0..(point - n) {
                out.push('0');
            }
        } else {
            let split = point as usize;
            out.push_str(&digits[..split]);
            out.push('.');
            out.push_str(&digits[split..]);
        }
    } else {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        let exponent = point - 1;
        out.push('e');
        if exponent >= 0 {
            out.push('+');
        }
        out.push_str(&exponent.to_string());
    }
    out
}

fn write_canonical(value: &serde_json::Value, out: &mut String) -> Result<(), String> {
    use serde_json::Value;
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                out.push_str(&i.to_string());
            } else if let Some(u) = n.as_u64() {
                out.push_str(&u.to_string());
            } else if let Some(f) = n.as_f64() {
                out.push_str(&js_number(f));
            } else {
                return Err("unsupported JSON number".to_string());
            }
        }
        Value::String(s) => {
            let quoted = serde_json::to_string(s).map_err(|e| e.to_string())?;
            out.push_str(&quoted);
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out)?;
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for (i, key) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                let quoted = serde_json::to_string(key).map_err(|e| e.to_string())?;
                out.push_str(&quoted);
                out.push(':');
                write_canonical(&map[key], out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

pub fn canonical_json(value: &serde_json::Value) -> Result<String, String> {
    let mut out = String::new();
    write_canonical(value, &mut out)?;
    Ok(out)
}

pub fn canonical_hash(value: &serde_json::Value) -> Result<String, String> {
    Ok(sha256(canonical_json(value)?.as_bytes()))
}

/// SHA-256 of every decompressed ZIP entry, keyed by entry name.
pub fn entry_hashes(path: &Path) -> Result<BTreeMap<String, String>, String> {
    let file = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let archive = ZipArchive::new(file).map_err(|e| format!("{}: {e}", path.display()))?;
    collect_entry_hashes(archive, &path.display().to_string())
}

/// `entry_hashes` for package bytes that are not on disk.
pub fn entry_hashes_from_bytes(bytes: &[u8]) -> Result<BTreeMap<String, String>, String> {
    let archive = ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    collect_entry_hashes(archive, "<memory>")
}

fn collect_entry_hashes<R: std::io::Read + std::io::Seek>(
    mut archive: ZipArchive<R>,
    label: &str,
) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| format!("{}: {e}", label))?;
        let name = entry
            .name()
            .map_err(|e| format!("{}: {e}", label))?
            .to_string();
        let mut hasher = Sha256::new();
        let mut buf = [0u8; 64 * 1024];
        loop {
            let read = entry
                .read(&mut buf)
                .map_err(|e| format!("{}: {e}", label))?;
            if read == 0 {
                break;
            }
            hasher.update(&buf[..read]);
        }
        out.insert(name, hex_digest(&hasher.finalize()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_number_matches_javascript_json() {
        assert_eq!(js_number(0.0), "0");
        assert_eq!(js_number(-0.0), "0");
        assert_eq!(js_number(211.20000000000002), "211.20000000000002");
        assert_eq!(
            js_number(0.000006106100472931994),
            "0.000006106100472931994"
        );
        assert_eq!(js_number(1e-7), "1e-7");
        assert_eq!(js_number(1e21), "1e+21");
        assert_eq!(js_number(-3.5), "-3.5");
    }

    #[test]
    fn canonical_json_sorts_keys_and_keeps_array_order() {
        let value = serde_json::json!({"b": 1, "a": [3, 2, 1]});
        assert_eq!(canonical_json(&value).unwrap(), r#"{"a":[3,2,1],"b":1}"#);
    }

    #[test]
    fn canonical_hash_sorts_keys() {
        let a = serde_json::json!({"b": 2, "a": 1});
        let b = serde_json::json!({"a": 1, "b": 2});
        assert_eq!(canonical_hash(&a).unwrap(), canonical_hash(&b).unwrap());
    }
}
