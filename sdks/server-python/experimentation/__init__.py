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

import os
import sys
import warnings

warnings.warn(
    "Importing from the flat 'experimentation/__init__.py' package is deprecated. "
    "Use 'from experimentation.client import ExperimentClient' and "
    "'from experimentation.providers import RemoteProvider' instead. "
    "See sdks/server-python/src/experimentation/ for the canonical package.",
    DeprecationWarning,
    stacklevel=2,
)

# The canonical modular package lives under src/experimentation/.  We cannot
# use a plain ``from experimentation.client import …`` because Python would
# resolve *this* package (experimentation/) instead of src/experimentation/.
# Temporarily prepend src/ to sys.path so the import targets the canonical
# modules, then restore the original path.
_src_dir = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "src")
sys.path.insert(0, _src_dir)
try:
    from importlib import import_module as _imp

    _client_mod = _imp("experimentation.client")
    _providers_mod = _imp("experimentation.providers")
    _types_mod = _imp("experimentation.types")
finally:
    sys.path.remove(_src_dir)

ExperimentClient = _client_mod.ExperimentClient
AssignmentProvider = _providers_mod.AssignmentProvider
LocalProvider = _providers_mod.LocalProvider
MockProvider = _providers_mod.MockProvider
RemoteProvider = _providers_mod.RemoteProvider
Assignment = _types_mod.Assignment
ExperimentConfig = _types_mod.ExperimentConfig
UserAttributes = _types_mod.UserAttributes
VariantConfig = _types_mod.VariantConfig

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
