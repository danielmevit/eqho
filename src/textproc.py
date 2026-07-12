"""Text post-processing between transcription and injection — all local.

Stages (each opt-in via settings):
- voice commands: a whole utterance like "new line" or "period" becomes the
  symbol; "delete that" removes the previous utterance
- replacements: user-defined word/phrase substitutions (word-boundary,
  case-insensitive)
- smart join: punctuation and newline segments attach without a leading space
- clean_text: light-touch deterministic formatting cleanup (casing + spacing;
  optional filler removal). Tier 1 only — NEVER rephrases or reorders words.
  (LLM "polish" that could reword the transcript is deliberately NOT here —
  parked on the roadmap; dictation must return exactly what you said.)
"""

import re

# Whole-utterance commands → what they produce. English v1.
VOICE_COMMANDS = {
    "new line": "\n",
    "new paragraph": "\n\n",
    "period": ".",
    "full stop": ".",
    "comma": ",",
    "question mark": "?",
    "exclamation mark": "!",
    "exclamation point": "!",
    "colon": ":",
    "semicolon": ";",
}

DELETE_COMMANDS = {"delete that", "scratch that"}

_NO_LEADING_SPACE = {".", ",", "?", "!", ":", ";"}


def normalize(text: str) -> str:
    """Lowercased utterance with edge punctuation stripped, for command matching."""
    return text.strip().lower().strip(".,!?;: ")


def match_command(text: str):
    """Return the replacement text if the WHOLE utterance is a voice command,
    else None."""
    return VOICE_COMMANDS.get(normalize(text))


def is_delete_command(text: str) -> bool:
    return normalize(text) in DELETE_COMMANDS


def apply_replacements(text: str, replacements: dict) -> str:
    """User-defined substitutions, word-boundary and case-insensitive."""
    for src, dst in (replacements or {}).items():
        if not src:
            continue
        try:
            text = re.sub(rf"\b{re.escape(src)}\b", dst, text, flags=re.IGNORECASE)
        except re.error:
            continue
    return text


def smart_join(parts: list) -> str:
    """Join utterance segments: normal words get a space between them,
    punctuation attaches to the previous word, newlines swallow the space."""
    out = ""
    for part in parts:
        part = part.strip() if part not in ("\n", "\n\n") else part
        if not part:
            continue
        if not out:
            out = part
        elif part in _NO_LEADING_SPACE or part.startswith("\n"):
            out += part
        elif out.endswith("\n"):
            out += part
        else:
            out += " " + part
    return out


# -- Tier-1 formatting cleanup ------------------------------------------------
# Deterministic, safe, and idempotent. Fixes the mechanical grime in Whisper
# output (chunk-seam casing, stray spacing) without ever changing your words.

# Conservative filler list — only removed when the user opts in. Deliberately
# excludes ambiguous words ("like", "so", "well", "right") that carry meaning.
_FILLERS = ("um", "umm", "uh", "uhh", "er", "erm", "hmm", "mm", "mmm")
_FILLER_RE = re.compile(r"\b(?:" + "|".join(_FILLERS) + r")\b[ \t]*,?", re.IGNORECASE)


def _strip_fillers(text: str) -> str:
    return _FILLER_RE.sub("", text)


def _fix_i(text: str) -> str:
    """Lone 'i' and its contractions → 'I' (leaves words like 'iOS' alone)."""
    text = re.sub(r"\bi\b", "I", text)
    text = re.sub(r"\bi('m|'ll|'ve|'d)\b", lambda m: "I" + m.group(1), text)
    return text


def _capitalize_sentences(text: str) -> str:
    """Capitalize the first letter of the text and of each sentence (after
    . ! ? or a newline). Casing only — never touches the words themselves."""
    text = re.sub(r"^(\s*)([a-z])", lambda m: m.group(1) + m.group(2).upper(), text)
    text = re.sub(r"([.!?][ \t]+)([a-z])",
                  lambda m: m.group(1) + m.group(2).upper(), text)
    text = re.sub(r"(\n[ \t]*)([a-z])",
                  lambda m: m.group(1) + m.group(2).upper(), text)
    return text


def clean_text(text: str, remove_fillers: bool = False) -> str:
    """Light-touch formatting cleanup. Safe and idempotent."""
    if not text:
        return text
    if remove_fillers:
        text = _strip_fillers(text)
    # Whitespace: collapse runs of spaces/tabs (NOT newlines), trim line edges
    text = re.sub(r"[ \t]+", " ", text)
    text = re.sub(r"[ \t]+\n", "\n", text)
    text = re.sub(r"\n[ \t]+", "\n", text)
    # No space before ,.!?;: (newlines preserved)
    text = re.sub(r"[ \t]+([,.!?;:])", r"\1", text)
    # Drop any leading punctuation/space a filler strip may have left behind
    text = re.sub(r"^[\s,;:]+", "", text)
    text = _capitalize_sentences(text)
    text = _fix_i(text)
    return text.strip()
