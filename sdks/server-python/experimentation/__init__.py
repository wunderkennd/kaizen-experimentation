"""
Experimentation Platform — Python Server SDK

Implements the Provider Abstraction pattern (ADR-007) with three backends:
  - RemoteProvider: Calls the Assignment Service via ConnectRPC/gRPC
  - LocalProvider:  Evaluates assignments locally using cached config
  - MockProvider:   Returns deterministic assignments for testing

Usage:
    from experimentation import ExperimentClient, RemoteProvider

    client = ExperimentClient(
        provider=RemoteProvider(base_url="https://assignment.example.com"),
    )
    variant = await client.get_variant("homepage_recs_v2", user_id="user-123")
"""

from __future__ import annotations

from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional

import mmh3


# ---------------------------------------------------------------------------
# Core Types
# ---------------------------------------------------------------------------

@dataclass
class Assignment:
    """A variant assignment for a single experiment."""
    experiment_id: str
    variant_name: str
    payload: Dict[str, Any] = field(default_factory=dict)
    from_cache: bool = False


@dataclass
class UserAttributes:
    """User attributes for targeting evaluation."""
    user_id: str
    properties: Dict[str, Any] = field(default_factory=dict)


@dataclass
class VariantConfig:
    """Variant-level configuration."""
    name: str
    traffic_fraction: float
    is_control: bool = False
    payload: Dict[str, Any] = field(default_factory=dict)


@dataclass
class ExperimentConfig:
    """Configuration for local assignment evaluation."""
    experiment_id: str
    hash_salt: str
    layer_name: str
    variants: List[VariantConfig]
    allocation_start: int
    allocation_end: int
    total_buckets: int = 10000


# ---------------------------------------------------------------------------
# Provider Interface
# ---------------------------------------------------------------------------

class AssignmentProvider(ABC):
    """
    Provider abstraction — all assignment backends implement this interface.
    See ADR-007 for the design rationale.
    """

    @abstractmethod
    async def initialize(self) -> None:
        """Initialize the provider (establish connections, fetch config)."""
        ...

    @abstractmethod
    async def get_assignment(
        self, experiment_id: str, attrs: UserAttributes
    ) -> Optional[Assignment]:
        """Get a variant assignment for the given experiment and user."""
        ...

    @abstractmethod
    async def get_all_assignments(
        self, attrs: UserAttributes
    ) -> Dict[str, Assignment]:
        """Get assignments for all active experiments."""
        ...

    @abstractmethod
    async def close(self) -> None:
        """Shut down the provider and release resources."""
        ...


# ---------------------------------------------------------------------------
# RemoteProvider
# ---------------------------------------------------------------------------

class RemoteProvider(AssignmentProvider):
    """Calls the Assignment Service via ConnectRPC."""

    def __init__(self, base_url: str, timeout_ms: int = 2000) -> None:
        self.base_url = base_url
        self.timeout_ms = timeout_ms

    async def initialize(self) -> None:
        # TODO (Agent-1): Create ConnectRPC/gRPC channel
        pass

    async def get_assignment(
        self, experiment_id: str, attrs: UserAttributes
    ) -> Optional[Assignment]:
        # TODO (Agent-1): Call AssignmentService.GetAssignment
        return None

    async def get_all_assignments(
        self, attrs: UserAttributes
    ) -> Dict[str, Assignment]:
        # TODO (Agent-1): Call AssignmentService.GetAllAssignments
        return {}

    async def close(self) -> None:
        # TODO (Agent-1): Close channel
        pass


# ---------------------------------------------------------------------------
# LocalProvider
# ---------------------------------------------------------------------------

class LocalProvider(AssignmentProvider):
    """Evaluates assignments locally using cached config."""

    def __init__(self, configs: List[ExperimentConfig]) -> None:
        self._experiments = {c.experiment_id: c for c in configs}

    async def initialize(self) -> None:
        pass

    async def get_assignment(
        self, experiment_id: str, attrs: UserAttributes
    ) -> Optional[Assignment]:
        config = self._experiments.get(experiment_id)
        if config is None:
            return None
        if not config.variants:
            return None

        key = f"{attrs.user_id}\x00{config.hash_salt}"
        raw_hash: int = mmh3.hash(key, seed=0, signed=False)
        bucket = raw_hash % config.total_buckets

        if bucket < config.allocation_start or bucket > config.allocation_end:
            return None

        alloc_size = float(config.allocation_end - config.allocation_start + 1)
        relative_bucket = float(bucket - config.allocation_start)

        cumulative = 0.0
        for variant in config.variants:
            cumulative += variant.traffic_fraction * alloc_size
            if relative_bucket < cumulative:
                return Assignment(
                    experiment_id=config.experiment_id,
                    variant_name=variant.name,
                    payload=variant.payload,
                    from_cache=True,
                )

        # FP rounding fallback — assign to last variant
        last = config.variants[-1]
        return Assignment(
            experiment_id=config.experiment_id,
            variant_name=last.name,
            payload=last.payload,
            from_cache=True,
        )

    async def get_all_assignments(
        self, attrs: UserAttributes
    ) -> Dict[str, Assignment]:
        results: Dict[str, Assignment] = {}
        for exp_id in self._experiments:
            assignment = await self.get_assignment(exp_id, attrs)
            if assignment is not None:
                results[exp_id] = assignment
        return results

    async def close(self) -> None:
        self._experiments.clear()


# ---------------------------------------------------------------------------
# MockProvider
# ---------------------------------------------------------------------------

class MockProvider(AssignmentProvider):
    """Returns deterministic assignments for testing."""

    def __init__(self, assignments: Optional[Dict[str, str]] = None) -> None:
        self._assignments: Dict[str, Assignment] = {}
        for exp_id, variant in (assignments or {}).items():
            self._assignments[exp_id] = Assignment(
                experiment_id=exp_id, variant_name=variant
            )

    async def initialize(self) -> None:
        pass

    async def get_assignment(
        self, experiment_id: str, attrs: UserAttributes
    ) -> Optional[Assignment]:
        return self._assignments.get(experiment_id)

    async def get_all_assignments(
        self, attrs: UserAttributes
    ) -> Dict[str, Assignment]:
        return dict(self._assignments)

    def set_assignment(
        self, experiment_id: str, variant_name: str, payload: Optional[Dict[str, Any]] = None
    ) -> None:
        """Override an assignment at runtime (useful in tests)."""
        self._assignments[experiment_id] = Assignment(
            experiment_id=experiment_id,
            variant_name=variant_name,
            payload=payload or {},
        )

    async def close(self) -> None:
        self._assignments.clear()


# ---------------------------------------------------------------------------
# Client
# ---------------------------------------------------------------------------

class ExperimentClient:
    """Main entry point for the Experimentation SDK."""

    def __init__(
        self,
        provider: AssignmentProvider,
        fallback_provider: Optional[AssignmentProvider] = None,
    ) -> None:
        self._provider = provider
        self._fallback = fallback_provider
        self._initialized = False

    async def initialize(self) -> None:
        await self._provider.initialize()
        if self._fallback:
            await self._fallback.initialize()
        self._initialized = True

    async def get_variant(
        self, experiment_id: str, user_id: str, properties: Optional[Dict[str, Any]] = None
    ) -> Optional[str]:
        assignment = await self.get_assignment(experiment_id, user_id, properties)
        return assignment.variant_name if assignment else None

    async def get_assignment(
        self, experiment_id: str, user_id: str, properties: Optional[Dict[str, Any]] = None
    ) -> Optional[Assignment]:
        if not self._initialized:
            await self.initialize()

        attrs = UserAttributes(user_id=user_id, properties=properties or {})

        try:
            return await self._provider.get_assignment(experiment_id, attrs)
        except Exception:
            if self._fallback:
                return await self._fallback.get_assignment(experiment_id, attrs)
            return None

    async def close(self) -> None:
        await self._provider.close()
        if self._fallback:
            await self._fallback.close()
        self._initialized = False
