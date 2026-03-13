"""
Assignment providers implementing the Provider Abstraction (ADR-007).

Three backends:
  - RemoteProvider:  ConnectRPC calls to the Assignment Service
  - LocalProvider:   Local evaluation using cached config + hash
  - MockProvider:    Deterministic assignments for testing
"""

from __future__ import annotations

import abc
from typing import Any

import mmh3

from experimentation.types import Assignment, ExperimentConfig, UserAttributes


class AssignmentProvider(abc.ABC):
    """Base interface for assignment backends. See ADR-007."""

    @abc.abstractmethod
    async def initialize(self) -> None:
        """Prepare the provider (fetch config, establish connections)."""

    @abc.abstractmethod
    async def get_assignment(
        self, experiment_id: str, attrs: UserAttributes
    ) -> Assignment | None:
        """Get a variant assignment. Returns None if user is not in experiment."""

    @abc.abstractmethod
    async def get_all_assignments(
        self, attrs: UserAttributes
    ) -> dict[str, Assignment]:
        """Get assignments for all active experiments."""

    @abc.abstractmethod
    async def close(self) -> None:
        """Shut down and release resources."""


class RemoteProvider(AssignmentProvider):
    """Calls the Assignment Service via ConnectRPC over HTTP."""

    def __init__(self, base_url: str, timeout_ms: int = 2000) -> None:
        self._base_url = base_url.rstrip("/")
        self._timeout_ms = timeout_ms
        self._client: Any = None

    async def initialize(self) -> None:
        # TODO (Agent-1): Create httpx.AsyncClient with ConnectRPC transport
        import httpx

        self._client = httpx.AsyncClient(
            base_url=self._base_url,
            timeout=self._timeout_ms / 1000.0,
        )

    async def get_assignment(
        self, experiment_id: str, attrs: UserAttributes
    ) -> Assignment | None:
        # TODO (Agent-1): POST to AssignmentService/GetAssignment
        _ = experiment_id, attrs
        return None

    async def get_all_assignments(
        self, attrs: UserAttributes
    ) -> dict[str, Assignment]:
        # TODO (Agent-1): POST to AssignmentService/GetAllAssignments
        _ = attrs
        return {}

    async def close(self) -> None:
        if self._client:
            await self._client.aclose()


class LocalProvider(AssignmentProvider):
    """Evaluates assignments locally using cached experiment configs."""

    def __init__(self, configs: list[ExperimentConfig]) -> None:
        self._experiments = {c.experiment_id: c for c in configs}

    async def initialize(self) -> None:
        pass  # Static config — nothing to do

    async def get_assignment(
        self, experiment_id: str, attrs: UserAttributes
    ) -> Assignment | None:
        config = self._experiments.get(experiment_id)
        if config is None:
            return None
        if not config.variants:
            return None

        key = f"{attrs.user_id}\x00{config.hash_salt}"
        raw_hash = mmh3.hash(key, seed=0, signed=False)
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
    ) -> dict[str, Assignment]:
        results: dict[str, Assignment] = {}
        for eid in self._experiments:
            a = await self.get_assignment(eid, attrs)
            if a is not None:
                results[eid] = a
        return results

    async def close(self) -> None:
        self._experiments.clear()


class MockProvider(AssignmentProvider):
    """Returns deterministic assignments for testing."""

    def __init__(
        self, assignments: dict[str, Assignment] | None = None
    ) -> None:
        self._assignments: dict[str, Assignment] = assignments or {}

    async def initialize(self) -> None:
        pass

    async def get_assignment(
        self, experiment_id: str, attrs: UserAttributes
    ) -> Assignment | None:
        _ = attrs
        return self._assignments.get(experiment_id)

    async def get_all_assignments(
        self, attrs: UserAttributes
    ) -> dict[str, Assignment]:
        _ = attrs
        return dict(self._assignments)

    def set_assignment(
        self, experiment_id: str, variant_name: str
    ) -> None:
        """Override an assignment at runtime (useful in tests)."""
        self._assignments[experiment_id] = Assignment(
            experiment_id=experiment_id,
            variant_name=variant_name,
        )

    async def close(self) -> None:
        self._assignments.clear()
