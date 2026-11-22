//! Deterministic text post-processing between transcription and injection.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::{NoExpand, Regex, RegexBuilder};

pub const VOICE_COMMANDS: [(&str, &str); 10] = [
    ("new line", "\n"),
    ("new paragraph", "\n\n"),
    ("period", "."),
    ("full stop", "."),
    ("comma", ","),
    ("question mark", "?"),
    ("exclamation mark", "!"),
    ("exclamation point", "!"),
    ("colon", ":"),
    ("semicolon", ";"),
];

pub const DELETE_COMMANDS: [&str; 2] = ["delete that", "scratch that"];

const NO_LEADING_SPACE: [&str; 6] = [".", ",", "?", "!", ":", ";"];

pub fn normalize(text: &str) -> String {
    text.trim()
        .to_lowercase()
        .trim_matches(|c: char| matches!(c, '.' | ',' | '!' | '?' | ';' | ':' | ' '))
        .to_owned()
}

pub fn match_command(text: &str) -> Option<&'static str> {
    let norm = normalize(text);
    VOICE_COMMANDS
        .iter()
        .find(|(cmd, _)| *cmd == norm)
        .map(|(_, repl)| *repl)
}

pub fn is_delete_command(text: &str) -> bool {
    let norm = normalize(text);
    DELETE_COMMANDS.contains(&norm.as_str())
}

/// Case-insensitive user-defined substitutions using literal replacement text
/// (never interpreting `$1`/`\1`) and smart word-boundary matching so
/// punctuation-bearing terms like `"C++"` or `".NET"` still match.
pub fn apply_replacements(text: &str, replacements: &BTreeMap<String, String>) -> String {
    let mut out = text.to_owned();
    for (src, dst) in replacements {
        if src.is_empty() {
            continue;
        }
        let first_is_word = src
            .chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || c == '_');
        let last_is_word = src
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_');

        let mut pattern = String::with_capacity(src.len() + 8);
        if first_is_word {
            pattern.push_str(r"\b");
        }
        pattern.push_str(&regex::escape(src));
        if last_is_word {
            pattern.push_str(r"\b");
        }

        if let Ok(re) = RegexBuilder::new(&pattern).case_insensitive(true).build() {
            out = re.replace_all(&out, NoExpand(dst.as_str())).into_owned();
        }
    }
    out
}

pub fn smart_join(parts: &[&str]) -> String {
    let mut out = String::new();
    for &raw_part in parts {
        let part = if raw_part == "\n" || raw_part == "\n\n" {
            raw_part
        } else {
            raw_part.trim()
        };
        if part.is_empty() {
            continue;
        }
        if out.is_empty()
            || NO_LEADING_SPACE.contains(&part)
            || part.starts_with('\n')
            || out.ends_with('\n')
        {
            out.push_str(part);
        } else {
            out.push(' ');
            out.push_str(part);
        }
    }
    out
}

static FILLER_RE: LazyLock<Option<Regex>> = LazyLock::new(|| {
    RegexBuilder::new(r"\b(?:um|umm|uh|uhh|er|erm|hmm|mm|mmm)\b[ \t]*,?")
        .case_insensitive(true)
        .build()
        .ok()
});
static SPACES_RE: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"[ \t]+").ok());
static SPACE_BEFORE_NL_RE: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"[ \t]+\n").ok());
static SPACE_AFTER_NL_RE: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"\n[ \t]+").ok());
static SPACE_BEFORE_PUNCT_RE: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"[ \t]+([,.!?;:])").ok());
static LEADING_PUNCT_RE: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^[\s,;:]+").ok());
static LONE_I_RE: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"\bi\b").ok());
static I_CONTRACTIONS_RE: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"\bi('m|'ll|'ve|'d)\b").ok());

fn capitalize_sentences_unicode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut cap_next = true;
    let mut saw_sentence_end = false;

    for ch in text.chars() {
        if cap_next {
            if ch.is_alphabetic() {
                for up in ch.to_uppercase() {
                    out.push(up);
                }
                cap_next = false;
                saw_sentence_end = false;
                continue;
            }
            if !ch.is_whitespace() {
                // Non-letter, non-whitespace (e.g., digit or quote) cancels
                // the pending sentence-start capitalization.
                cap_next = false;
            }
        }

        out.push(ch);

        if ch == '\n' {
            cap_next = true;
            saw_sentence_end = false;
        } else if matches!(ch, '.' | '!' | '?') {
            saw_sentence_end = true;
        } else if saw_sentence_end && matches!(ch, ' ' | '\t') {
            cap_next = true;
            saw_sentence_end = false;
        } else {
            saw_sentence_end = false;
        }
    }

    out
}

/// Language-aware light-touch deterministic formatting cleanup.
///
/// English filler stripping and lone `i` / `i'm` / `i'll` / `i've` / `i'd`
/// capitalization run ONLY when `language.eq_ignore_ascii_case("en")`.
/// Strictly idempotent.
pub fn clean_text(text: &str, language: &str, remove_fillers: bool) -> String {
    if text.is_empty() {
        return String::new();
    }

    let is_en = language.eq_ignore_ascii_case("en");
    let mut out = text.to_owned();

    if remove_fillers && is_en && let Some(re) = &*FILLER_RE {
        out = re.replace_all(&out, "").into_owned();
    }
    if let Some(re) = &*SPACES_RE {
        out = re.replace_all(&out, " ").into_owned();
    }
    if let Some(re) = &*SPACE_BEFORE_NL_RE {
        out = re.replace_all(&out, "\n").into_owned();
    }
    if let Some(re) = &*SPACE_AFTER_NL_RE {
        out = re.replace_all(&out, "\n").into_owned();
    }
    if let Some(re) = &*SPACE_BEFORE_PUNCT_RE {
        out = re.replace_all(&out, "$1").into_owned();
    }
    if let Some(re) = &*LEADING_PUNCT_RE {
        out = re.replace(&out, "").into_owned();
    }

    out = capitalize_sentences_unicode(&out);

    if is_en {
        if let Some(re) = &*LONE_I_RE {
            out = re.replace_all(&out, "I").into_owned();
        }
        if let Some(re) = &*I_CONTRACTIONS_RE {
            out = re.replace_all(&out, "I$1").into_owned();
        }
    }

    out.trim().to_owned()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn smoke_invariants() {
        assert_eq!(match_command("Period."), Some("."));
        assert_eq!(match_command("New line"), Some("\n"));
        assert_eq!(match_command("hello world"), None);
        assert!(is_delete_command("Delete that"));
        assert!(is_delete_command("scratch that!"));

        let mut map = BTreeMap::new();
        map.insert("eqho".to_owned(), "Eqho".to_owned());
        assert_eq!(apply_replacements("hello eqho app", &map), "hello Eqho app");

        assert_eq!(smart_join(&["hello", ".", "world"]), "hello. world");
        assert_eq!(smart_join(&["one", "\n", "two"]), "one\ntwo");

        assert_eq!(clean_text("hello world", "en", false), "Hello world");
        assert_eq!(
            clean_text("i think i'm right . yes", "en", false),
            "I think I'm right. Yes"
        );
        assert_eq!(clean_text("hello  ,  world", "en", false), "Hello, world");
        assert_eq!(clean_text("um so uh hi", "en", true), "So hi");
        assert_eq!(clean_text("um so uh hi", "en", false), "Um so uh hi");
        assert_eq!(clean_text("one\ntwo", "en", false), "One\nTwo");
        assert_eq!(
            clean_text(&clean_text("i said hi", "en", false), "en", false),
            clean_text("i said hi", "en", false)
        );
    }

    #[test]
    fn literal_and_punctuation_replacements() {
        let mut map = BTreeMap::new();
        map.insert("C++".to_owned(), "Rust $1 \\1".to_owned());
        map.insert(".NET".to_owned(), "dotnet".to_owned());
        assert_eq!(
            apply_replacements("I write c++ and .net code.", &map),
            "I write Rust $1 \\1 and dotnet code."
        );
    }

    #[test]
    fn non_english_clean_text_preserves_fillers_and_i_and_unicode_caps() {
        // Spanish "i" / "um" or French/German/Cyrillic should not be mangled by English rules
        assert_eq!(clean_text("um i test. äpfel! жёлтый\nмёд", "de", true), "Um i test. Äpfel! Жёлтый\nМёд");
        assert_eq!(clean_text("écrire bien", "fr", true), "Écrire bien");
    }

    proptest! {
        #[test]
        fn clean_text_is_idempotent(
            s in ".*",
            lang in prop_oneof![Just("en"), Just("de"), Just("fr"), Just("ru")],
            fillers in any::<bool>()
        ) {
            let once = clean_text(&s, lang, fillers);
            let twice = clean_text(&once, lang, fillers);
            prop_assert_eq!(twice, once);
        }
    }
}
