"""Background Whisper-model downloads with progress estimation.

faster-whisper/huggingface_hub don't expose a progress callback, so progress
is estimated by polling the on-disk size of the model's cache folder against
the known model size. Good enough for a smooth 0→100 bar.
"""

import logging
import os
import threading
from pathlib import Path

log = logging.getLogger(__name__)

# Approximate on-disk sizes (MB) for progress estimation
MODEL_SIZES_MB = {
    "distil-large-v3": 1510,
    "distil-medium.en": 750,
    "distil-small.en": 330,
    "large-v3-turbo": 1620,
    "medium": 1530,
    "small": 970,
    "base": 290,
    "tiny": 150,
    "large-v3": 3090,
}

# model_key -> {"progress": float 0..1, "done": bool, "error": str|None}
_downloads: dict = {}
_lock = threading.Lock()


def status(model_key: str) -> dict:
    with _lock:
        return dict(_downloads.get(model_key) or {})


def is_downloading(model_key: str) -> bool:
    s = status(model_key)
    return bool(s) and not s.get("done")


def _engine(settings) -> str:
    from .model_host import resolve_backend  # inside: avoids an import cycle
    return resolve_backend(settings.engine_backend)


def model_files(settings, model_key: str) -> list:
    """On-disk files/folders of `model_key` for the engine in use."""
    model_dir = settings.resolve_model_dir()
    if _engine(settings) == "whisper.cpp":
        from .model_host import whispercpp_model_path
        path = whispercpp_model_path(model_dir, model_key)
        return [path] if path.exists() else []
    # Exact repo names: a glob on the key would make "small.en" also match
    # "distil-small.en" (and delete_model would remove both).
    names = (model_key,
             f"models--Systran--faster-whisper-{model_key}",
             f"models--Systran--faster-distil-whisper-{model_key.removeprefix('distil-')}")
    if not model_key.startswith("distil-"):
        names = names[:2]
    return [root / n for root in (model_dir, model_dir / "huggingface")
            for n in names if (root / n).exists()]


def model_size_mb(settings, model_key: str) -> float:
    """Downloaded size of `model_key` for the engine in use (0 if absent)."""
    total = 0
    for item in model_files(settings, model_key):
        files = [item] if item.is_file() else item.rglob("*")
        for f in files:
            try:
                if f.is_file():
                    total += f.stat().st_size
            except OSError:
                continue
    return total / 1e6


def delete_model(settings, model_key: str) -> bool:
    """Remove `model_key`'s files for the engine in use. True if anything went."""
    import shutil
    removed = False
    for item in model_files(settings, model_key):
        try:
            if item.is_dir():
                shutil.rmtree(item)
            else:
                item.unlink()
            removed = True
        except OSError as e:
            log.error("Could not delete %s: %s", item, e)
    return removed


def _dir_size_mb(model_dir: Path, model_key: str) -> float:
    total = 0
    roots = [model_dir, model_dir / "huggingface"]
    candidates = []
    for root in roots:
        if root.exists():
            candidates.extend(root.glob(f"models--*{model_key}*"))
    direct = model_dir / model_key
    if direct.exists():
        candidates.append(direct)
    for folder in candidates:
        for f in folder.rglob("*"):
            try:
                if f.is_file():
                    total += f.stat().st_size
            except OSError:
                continue
    return total / 1e6


def start_download(settings, model_key: str) -> bool:
    """Kick off a background download. Returns False if one is already running."""
    with _lock:
        current = _downloads.get(model_key)
        if current and not current.get("done"):
            return False
        _downloads[model_key] = {"progress": 0.0, "done": False, "error": None}

    model_dir = settings.resolve_model_dir()
    expected_mb = MODEL_SIZES_MB.get(model_key, 1000)
    engine = _engine(settings)

    def _worker() -> None:
        stop = threading.Event()

        def _poll_size() -> None:
            while not stop.wait(0.5):
                if engine == "whisper.cpp":
                    size = model_size_mb(settings, model_key)
                else:
                    size = _dir_size_mb(model_dir, model_key)
                fraction = min(0.99, size / expected_mb)
                with _lock:
                    entry = _downloads.get(model_key)
                    if not entry or entry.get("done"):
                        return
                    entry["progress"] = fraction

        threading.Thread(target=_poll_size, daemon=True).start()
        try:
            model_dir.mkdir(parents=True, exist_ok=True)
            if engine == "whisper.cpp":
                from pywhispercpp.utils import download_model as ggml_download
                from .model_host import _WHISPERCPP_NAME, whispercpp_models_dir
                target = whispercpp_models_dir(model_dir)
                path = ggml_download(_WHISPERCPP_NAME.get(model_key, "base"),
                                     download_dir=str(target))
                if not path:
                    raise RuntimeError("whisper.cpp model download failed")
                with _lock:
                    _downloads[model_key].update(progress=1.0, done=True)
                log.info("Model %s (whisper.cpp) downloaded.", model_key)
                return
            os.environ["HF_HUB_CACHE"] = str(model_dir / "huggingface")
            try:
                from faster_whisper import download_model
            except ImportError:
                from faster_whisper.utils import download_model
            download_model(model_key, cache_dir=str(model_dir))
            with _lock:
                _downloads[model_key].update(progress=1.0, done=True)
            log.info("Model %s downloaded.", model_key)
        except Exception as e:
            log.error("Model download failed (%s): %s", model_key, e)
            with _lock:
                _downloads[model_key].update(done=True, error=str(e))
        finally:
            stop.set()

    threading.Thread(target=_worker, daemon=True).start()
    return True
