//! Recognise interesting Minecraft console lines.

use regex::Regex;
use std::sync::LazyLock;

#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    /// `Done (3.21s)! For help, type "help"`
    Ready,
    PlayerJoin(String),
    PlayerLeave(String),
    /// A crash report was written / fatal exception: treat the exit as a crash
    /// even if the JVM returns 0.
    Crash,
    /// Port already in use.
    BindFailed,
}

// Player names are anchored right after the log prefix "]: " so chat lines
// ("]: <Bob> Steve joined the game") cannot spoof joins. '.' covers Geyser/
// Floodgate bedrock prefixes.
static READY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"Done \([\d.,]+s\)! For help"#).unwrap());
static JOIN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\]: ([A-Za-z0-9_.*]{1,20}) joined the game$").unwrap());
static LEAVE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\]: ([A-Za-z0-9_.*]{1,20}) left the game$").unwrap());

pub fn parse(line: &str) -> Option<Event> {
    if READY.is_match(line) {
        return Some(Event::Ready);
    }
    if let Some(c) = JOIN.captures(line) {
        return Some(Event::PlayerJoin(c[1].to_string()));
    }
    if let Some(c) = LEAVE.captures(line) {
        return Some(Event::PlayerLeave(c[1].to_string()));
    }
    if line.contains("This crash report has been saved to")
        || line.contains("Encountered an unexpected exception")
    {
        return Some(Event::Crash);
    }
    if line.contains("FAILED TO BIND TO PORT") {
        return Some(Event::BindFailed);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_lines() {
        assert_eq!(
            parse(r#"[12:00:01] [Server thread/INFO]: Done (4.512s)! For help, type "help""#),
            Some(Event::Ready)
        );
        assert_eq!(
            parse("[12:00:01] [Server thread/INFO]: Steve joined the game"),
            Some(Event::PlayerJoin("Steve".into()))
        );
        assert_eq!(
            parse("[12:00:01 INFO]: .Bedrock_1 left the game"),
            Some(Event::PlayerLeave(".Bedrock_1".into()))
        );
        // chat spoofing attempt
        assert_eq!(parse("[12:00:01] [Server thread/INFO]: <Bob> Steve joined the game"), None);
        assert_eq!(parse("[12:00:01] [Server thread/WARN]: **** FAILED TO BIND TO PORT!"), Some(Event::BindFailed));
    }
}
