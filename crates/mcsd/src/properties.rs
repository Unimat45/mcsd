//! Lossless editing of `server.properties` (Java `.properties` format).
//!
//! Unchanged lines (comments, blank lines, untouched entries) are written back
//! byte for byte; only edited entries are re-serialised. Non-ASCII characters
//! are written as `\uXXXX` escapes, which every Minecraft version reads
//! correctly (older ones load the file as ISO-8859-1).

use crate::paths::write_atomic;
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::path::Path;

pub const FILE_NAME: &str = "server.properties";

#[derive(Clone, Debug)]
enum Line {
    /// Comment, blank line, or anything we don't touch.
    Verbatim(String),
    /// `raw` may span several physical lines (backslash continuations).
    Entry { key: String, value: String, raw: String },
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Property {
    pub key: String,
    pub value: String,
}

#[derive(Clone, Debug, Default)]
pub struct Properties {
    lines: Vec<Line>,
}

impl Properties {
    pub fn parse(text: &str) -> Self {
        let mut lines = Vec::new();
        let mut physical = text.lines().peekable();
        while let Some(first) = physical.next() {
            let trimmed = first.trim_start();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
                lines.push(Line::Verbatim(first.to_string()));
                continue;
            }
            // Join continuation lines: an odd number of trailing backslashes.
            let mut raw = first.to_string();
            let mut logical = trimmed.to_string();
            while ends_with_continuation(&logical) {
                logical.pop();
                match physical.next() {
                    Some(next) => {
                        raw.push('\n');
                        raw.push_str(next);
                        logical.push_str(next.trim_start());
                    }
                    None => break,
                }
            }
            let (key, value) = split_entry(&logical);
            lines.push(Line::Entry { key, value, raw });
        }
        Self { lines }
    }

    pub fn load(path: &Path) -> Result<Option<Self>> {
        match std::fs::read(path) {
            Ok(bytes) => Ok(Some(Self::parse(&decode(&bytes)))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        write_atomic(path, self.to_string().as_bytes())
    }

    /// Effective entries in file order. Like Java, a later duplicate wins
    /// (its value is reported at the first occurrence's position).
    pub fn entries(&self) -> Vec<Property> {
        let mut out: Vec<Property> = Vec::new();
        for line in &self.lines {
            if let Line::Entry { key, value, .. } = line {
                match out.iter_mut().find(|p| &p.key == key) {
                    Some(p) => p.value = value.clone(),
                    None => out.push(Property { key: key.clone(), value: value.clone() }),
                }
            }
        }
        out
    }

    pub fn get(&self, key: &str) -> Option<String> {
        self.lines.iter().rev().find_map(|l| match l {
            Line::Entry { key: k, value, .. } if k == key => Some(value.clone()),
            _ => None,
        })
    }

    /// Update every occurrence of `key` in place, or append it.
    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        validate_key(key)?;
        let raw = format!("{}={}", escape(key, true), escape(value, false));
        let mut found = false;
        for line in &mut self.lines {
            if let Line::Entry { key: k, value: v, raw: r } = line
                && k == key
            {
                if v != value {
                    *v = value.to_string();
                    *r = raw.clone();
                }
                found = true;
            }
        }
        if !found {
            self.lines.push(Line::Entry {
                key: key.to_string(),
                value: value.to_string(),
                raw,
            });
        }
        Ok(())
    }

    pub fn remove(&mut self, key: &str) {
        self.lines.retain(|l| !matches!(l, Line::Entry { key: k, .. } if k == key));
    }
}

impl std::fmt::Display for Properties {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for line in &self.lines {
            match line {
                Line::Verbatim(s) => writeln!(f, "{s}")?,
                Line::Entry { raw, .. } => writeln!(f, "{raw}")?,
            }
        }
        Ok(())
    }
}

/// Set one key in a properties file (created if missing).
pub fn set_in_file(path: &Path, key: &str, value: &str) -> Result<()> {
    let mut p = Properties::load(path)?.unwrap_or_default();
    p.set(key, value)?;
    p.save(path)
}

/// Read a file that may be UTF-8 (newer Minecraft) or ISO-8859-1 (older).
pub fn decode(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.trim_start_matches('\u{feff}').to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

/// Make user-supplied raw text safe for every Minecraft version: non-ASCII
/// characters become `\uXXXX` escapes (harmless inside comments too).
pub fn ascii_escape_raw(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        push_char(&mut out, c);
    }
    out
}

pub fn validate_key(key: &str) -> Result<()> {
    if key.is_empty() || key.len() > 128 {
        bail!("property names must be 1-128 characters");
    }
    if key.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '=' | ':' | '#' | '!' | '\\')) {
        bail!("invalid property name '{key}'");
    }
    Ok(())
}

fn ends_with_continuation(s: &str) -> bool {
    s.chars().rev().take_while(|&c| c == '\\').count() % 2 == 1
}

/// Split a logical line into (key, value), both unescaped. The key ends at
/// the first unescaped `=`, `:` or whitespace.
fn split_entry(line: &str) -> (String, String) {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 2,
            '=' | ':' | ' ' | '\t' | '\x0c' => break,
            _ => i += 1,
        }
    }
    let key_end = i.min(chars.len());
    let mut j = key_end;
    while j < chars.len() && matches!(chars[j], ' ' | '\t' | '\x0c') {
        j += 1;
    }
    if j < chars.len() && matches!(chars[j], '=' | ':') {
        j += 1;
        while j < chars.len() && matches!(chars[j], ' ' | '\t' | '\x0c') {
            j += 1;
        }
    }
    let key: String = chars[..key_end].iter().collect();
    let value: String = chars[j..].iter().collect();
    (unescape(&key), unescape(&value))
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('f') => out.push('\x0c'),
            Some('u') => {
                let hex: String = (0..4).filter_map(|_| it.next_if(|c| c.is_ascii_hexdigit())).collect();
                match u32::from_str_radix(&hex, 16) {
                    Ok(unit) if hex.len() == 4 => {
                        // Combine UTF-16 surrogate pairs (\uD83C\uDF32).
                        if (0xD800..0xDC00).contains(&unit) {
                            let mut look = it.clone();
                            if look.next() == Some('\\') && look.next() == Some('u') {
                                let low: String = (0..4).filter_map(|_| look.next_if(|c| c.is_ascii_hexdigit())).collect();
                                if let Ok(lo) = u32::from_str_radix(&low, 16)
                                    && low.len() == 4
                                    && (0xDC00..0xE000).contains(&lo)
                                {
                                    let cp = 0x10000 + ((unit - 0xD800) << 10) + (lo - 0xDC00);
                                    out.push(char::from_u32(cp).unwrap_or('\u{fffd}'));
                                    it = look;
                                    continue;
                                }
                            }
                        }
                        out.push(char::from_u32(unit).unwrap_or('\u{fffd}'));
                    }
                    _ => out.push_str(&hex), // malformed: keep the digits
                }
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

fn escape(s: &str, is_key: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, c) in s.chars().enumerate() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\x0c' => out.push_str("\\f"),
            '=' | ':' | '#' | '!' => {
                out.push('\\');
                out.push(c);
            }
            ' ' if is_key || i == 0 => out.push_str("\\ "),
            c => push_char(&mut out, c),
        }
    }
    out
}

fn push_char(out: &mut String, c: char) {
    if c.is_ascii() {
        out.push(c);
    } else {
        let mut buf = [0u16; 2];
        for unit in c.encode_utf16(&mut buf) {
            out.push_str(&format!("\\u{unit:04X}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "#Minecraft server properties\n#Sat Nov 07 15:34:24 EST 2026\nmotd=A Minecraft Server\nlevel-seed=\nrcon.password=p\\=a\\:ss\n\nmax-players = 20\n";

    #[test]
    fn parses_and_roundtrips_verbatim() {
        let p = Properties::parse(SAMPLE);
        assert_eq!(p.to_string(), SAMPLE);
        assert_eq!(p.get("rcon.password").as_deref(), Some("p=a:ss"));
        assert_eq!(p.get("max-players").as_deref(), Some("20"));
        assert_eq!(p.get("level-seed").as_deref(), Some(""));
        assert_eq!(p.entries().len(), 4);
    }

    #[test]
    fn edits_in_place_and_appends() {
        let mut p = Properties::parse(SAMPLE);
        p.set("max-players", "5").unwrap();
        p.set("pvp", "false").unwrap();
        p.remove("level-seed");
        let out = p.to_string();
        assert!(out.starts_with("#Minecraft server properties\n#Sat"));
        assert!(out.contains("motd=A Minecraft Server\nrcon.password=p\\=a\\:ss\n\nmax-players=5\npvp=false\n"));
        assert!(!out.contains("level-seed"));
    }

    #[test]
    fn escapes_special_values() {
        let mut p = Properties::default();
        p.set("motd", " Héllo: §aworld 🌲 #1").unwrap();
        let out = p.to_string();
        assert!(out.is_ascii());
        assert_eq!(out, "motd=\\ H\\u00E9llo\\: \\u00A7aworld \\uD83C\\uDF32 \\#1\n");
        // parsing our own output gives the original value back
        assert_eq!(Properties::parse(&out).get("motd").as_deref(), Some(" Héllo: §aworld 🌲 #1"));
    }

    #[test]
    fn handles_continuations_and_latin1() {
        let p = Properties::parse("motd=hello \\\n    world\n");
        assert_eq!(p.get("motd").as_deref(), Some("hello world"));
        assert_eq!(decode(b"motd=caf\xe9"), "motd=café");
        assert_eq!(ascii_escape_raw("# é\nmotd=é"), "# \\u00E9\nmotd=\\u00E9");
    }

    #[test]
    fn rejects_bad_keys() {
        assert!(validate_key("max-players").is_ok());
        assert!(validate_key("a b").is_err());
        assert!(validate_key("a=b").is_err());
        assert!(validate_key("").is_err());
    }
}
