"""Eqho dashboard UI package.

Public entry points re-exported from the orchestrator so callers can do
`from .ui import open_dashboard, shutdown_dashboard`.
"""

from .dashboard import open_dashboard, shutdown_dashboard

__all__ = ["open_dashboard", "shutdown_dashboard"]
