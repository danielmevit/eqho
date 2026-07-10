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

    def _worker() -> None:
        stop = threading.Event()

        def _poll_size() -> None:
            while not stop.wait(0.5):
                fraction = min(0.99, _dir_size_mb(model_dir, model_key) / expected_mb)
                with _lock:
                    entry = _downloads.get(model_key)
                    if not entry or entry.get("done"):
                        return
                    entry["progress"] = fraction

        threading.Thread(target=_poll_size, daemon=True).start()
        try:
            model_dir.mkdir(parents=True, exist_ok=True)
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
