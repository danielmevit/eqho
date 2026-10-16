"""Which speech model to suggest for this PC and language."""

_hw_class = None


def hardware_class() -> str:
    """"gpu" if the engine Auto picks runs on a graphics card (faster-whisper
    on NVIDIA, or whisper.cpp), else "cpu". Computed once per process."""
    global _hw_class
    if _hw_class is None:
        try:
            from . import model_host
            gpu = (model_host._cuda_available()
                   or model_host.resolve_backend("auto") == "whisper.cpp")
        except Exception:
            gpu = False
        _hw_class = "gpu" if gpu else "cpu"
    return _hw_class


def recommend_model(language: str) -> str:
    """Graphics card: the large models. Processor only: the small ones —
    distil-large-v3 needs ~10-19 s per sentence there (2026-10-16 benchmarks).
    English gets the English-only model; every other language the
    multilingual one."""
    if hardware_class() == "gpu":
        return "distil-large-v3" if language == "en" else "large-v3-turbo"
    return "distil-small.en" if language == "en" else "small"
