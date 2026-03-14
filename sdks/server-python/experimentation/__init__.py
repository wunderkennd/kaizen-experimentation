"""
Experimentation Platform — Python Server SDK (DEPRECATED legacy shim)

.. deprecated::
    This flat module is deprecated. Use the modular package instead::

        # NEW (canonical) — modular imports from src/experimentation/
        from experimentation.client import ExperimentClient
        from experimentation.providers import RemoteProvider, LocalProvider, MockProvider
        from experimentation.types import Assignment, UserAttributes

    This shim re-exports the canonical implementations so existing code
    continues to work, but will emit a DeprecationWarning on first import.
"""

from __future__ import annotations

import warnings

warnings.warn(
    "Importing from the flat 'experimentation/__init__.py' package is deprecated. "
    "Use 'from experimentation.client import ExperimentClient' and "
    "'from experimentation.providers import RemoteProvider' instead. "
    "See sdks/server-python/src/experimentation/ for the canonical package.",
    DeprecationWarning,
    stacklevel=2,
)

# Re-export from co-located submodules (copies of the canonical
# src/experimentation/ package).  These resolve naturally because
# client.py, providers.py, and types.py live alongside this __init__.py.
from experimentation.client import ExperimentClient  # noqa: E402,F401
from experimentation.providers import (  # noqa: E402,F401
    AssignmentProvider,
    LocalProvider,
    MockProvider,
    RemoteProvider,
)
from experimentation.types import (  # noqa: E402,F401
    Assignment,
    ExperimentConfig,
    UserAttributes,
    VariantConfig,
)

__all__ = [
    "Assignment",
    "AssignmentProvider",
    "ExperimentClient",
    "ExperimentConfig",
    "LocalProvider",
    "MockProvider",
    "RemoteProvider",
    "UserAttributes",
    "VariantConfig",
]
