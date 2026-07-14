"""Audio device enumeration and helpers using sounddevice."""

import logging
from typing import List, Tuple, Optional

import sounddevice as sd

log = logging.getLogger(__name__)


def list_input_devices() -> List[Tuple[int, str]]:
    """Return a list of (device_index, device_name) for input-capable devices."""
    devices = sd.query_devices()
    results = []
    for i, d in enumerate(devices):
        if d["max_input_channels"] > 0:
            results.append((i, d["name"]))
    return results


def get_default_input_device() -> Optional[int]:
    """Return the default input device index, or None."""
    try:
        info = sd.query_devices(kind="input")
        return info["index"] if isinstance(info, dict) else None
    except Exception:
        return None


def device_name(index: Optional[int]) -> str:
    """Human-readable name for a device index."""
    if index is None:
        return "System Default"
    try:
        return sd.query_devices(index)["name"]
    except Exception:
        return f"Device {index}"
