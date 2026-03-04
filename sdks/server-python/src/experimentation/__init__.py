"""
Experimentation Platform — Python Server SDK

Implements the Provider Abstraction pattern (ADR-007) with three backends:
  - RemoteProvider: Calls the Assignment Service via ConnectRPC/HTTP
  - LocalProvider:  Evaluates assignments locally using cached config
  - MockProvider:   Returns deterministic assignments for testing

Usage:
    from experimentation import ExperimentClient, RemoteProvider

    client = ExperimentClient(
        provider=RemoteProvider(base_url="https://assignment.example.com"),
        user_id="user-123",
    )
    variant = await client.get_variant("homepage_recs_v2")
"""

from experimentation.client import ExperimentClient
from experimentation.providers import (
    AssignmentProvider,
    LocalProvider,
    MockProvider,
    RemoteProvider,
)
from experimentation.types import Assignment, ExperimentConfig, UserAttributes, VariantConfig

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
