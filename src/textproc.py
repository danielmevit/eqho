"""Text post-processing between transcription and injection — all local.

Three stages, each opt-in via settings:
- voice commands: a whole utterance like "new line" or "period" becomes the
  symbol; "delete that" removes the previous utterance
- replacements: user-defined word/phrase substitutions (word-boundary,
  case-insensitive)
- smart join: punctuation and newline segments attach without a leading space
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
