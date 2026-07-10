"""OS abstraction layer — use `from .oskit import get` and call capabilities
on the returned kit. See base.py for the interface."""

from .base import OsKit, get

__all__ = ["OsKit", "get"]
