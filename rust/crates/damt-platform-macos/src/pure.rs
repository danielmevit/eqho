//! Pure cross-platform helpers for macOS integration (compiled and tested on all OSes).

use std::path::PathBuf;

/// Resolve `~/Library/LaunchAgents/<label>.plist`.
#[must_use]
pub fn launch_agent_plist_path(home: Option<&str>, label: &str) -> Option<PathBuf> {
    let h = home.map(str::trim).filter(|s| !s.is_empty())?;
    let clean: String = label
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .collect();
    if clean.is_empty() {
        return None;
    }
    Some(
        PathBuf::from(h)
            .join("Library")
            .join("LaunchAgents")
            .join(format!("{clean}.plist")),
    )
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

/// Generate a standard macOS per-user LaunchAgent plist launching `exe_path` at login.
#[must_use]
pub fn build_launch_agent_plist(label: &str, exe_path: &str) -> String {
    let lbl = xml_escape(label);
    let exe = xml_escape(exe_path);
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
         \t<key>Label</key>\n\
         \t<string>{lbl}</string>\n\
         \t<key>ProgramArguments</key>\n\
         \t<array>\n\
         \t\t<string>{exe}</string>\n\
         \t</array>\n\
         \t<key>RunAtLoad</key>\n\
         \t<true/>\n\
         \t<key>KeepAlive</key>\n\
         \t<false/>\n\
         </dict>\n\
         </plist>\n"
    )
}

/// Parse `defaults read -g AppleInterfaceStyle` output (`"Dark"` => dark, absent/empty => light).
#[must_use]
pub fn parse_apple_interface_style(raw: &str, exit_success: bool) -> Option<bool> {
    if exit_success && raw.trim().eq_ignore_ascii_case("dark") {
        Some(false) // dark
    } else {
        Some(true) // macOS deletes AppleInterfaceStyle when in Light mode
    }
}

/// Parse `defaults read com.apple.universalaccess reduceMotion` (`"1"` or `"true"`).
#[must_use]
pub fn parse_reduce_motion(raw: &str) -> bool {
    let t = raw.trim().to_ascii_lowercase();
    t == "1" || t == "true" || t == "yes"
}

/// Escape a string literal for AppleScript double-quoted strings.
#[must_use]
pub fn escape_applescript_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' | '\r' => out.push(' '),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn builds_valid_launch_agent_plist() {
        let p = launch_agent_plist_path(Some("/Users/dev"), "xyz.damt.eqho").unwrap();
        assert_eq!(
            p,
            PathBuf::from("/Users/dev/Library/LaunchAgents/xyz.damt.eqho.plist")
        );
        let xml = build_launch_agent_plist("xyz.damt.eqho", "/Applications/Eqho & Co.app/Contents/MacOS/Eqho");
        assert!(xml.contains("<string>xyz.damt.eqho</string>"));
        assert!(xml.contains("Eqho &amp; Co.app"));
        assert!(xml.contains("<key>RunAtLoad</key>"));
    }

    #[test]
    fn parses_macos_theme_and_motion() {
        assert_eq!(parse_apple_interface_style("Dark\n", true), Some(false));
        assert_eq!(parse_apple_interface_style("", false), Some(true));
        assert!(parse_reduce_motion("1\n"));
        assert!(!parse_reduce_motion("0\n"));
        assert_eq!(escape_applescript_string("Hi \"Eqho\"\nnow"), "Hi \\\"Eqho\\\" now");
    }
}
