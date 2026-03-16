"""Core types for the Experimentation SDK."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass(frozen=True)
class Assignment:
    """A variant assignment for a single experiment."""

    experiment_id: str
    variant_name: str
    payload: dict[str, Any] = field(default_factory=dict)
    from_cache: bool = False


@dataclass(frozen=True)
class UserAttributes:
    """User attributes for targeting evaluation."""

    user_id: str
    properties: dict[str, Any] = field(default_factory=dict)


@dataclass(frozen=True)
class VariantConfig:
    """Variant-level configuration."""

    name: str
    traffic_fraction: float
    is_control: bool = False
    payload: dict[str, Any] = field(default_factory=dict)


@dataclass(frozen=True)
class ExperimentConfig:
    """Full experiment configuration for local evaluation."""

    experiment_id: str
    hash_salt: str
    layer_name: str
    variants: list[VariantConfig]
    allocation_start: int
    allocation_end: int
    total_buckets: int = 10_000
