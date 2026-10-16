"""Which speech model to suggest for a language (first-time setup)."""


def recommend_model(language: str) -> str:
    """English gets the fast English-only model; every other language, and
    "detect automatically", gets the multilingual one."""
    return "distil-large-v3" if language == "en" else "large-v3-turbo"
