"""ExperimentClient — main entry point for the Python SDK."""

from __future__ import annotations

from typing import Any

from experimentation.providers import AssignmentProvider
from experimentation.types import Assignment, UserAttributes


class ExperimentClient:
    """
    High-level client for the Experimentation Platform.

    Wraps a primary provider with an optional fallback (ADR-007 chain).

    Usage:
        client = ExperimentClient(
            provider=RemoteProvider(base_url="https://assignment.example.com"),
            fallback_provider=LocalProvider(configs=[...]),
        )
        await client.initialize()
        variant = await client.get_variant("homepage_recs_v2", "user-123")
        await client.close()
    """

    def __init__(
        self,
        provider: AssignmentProvider,
        fallback_provider: AssignmentProvider | None = None,
    ) -> None:
        self._provider = provider
        self._fallback = fallback_provider
        self._initialized = False

    async def initialize(self) -> None:
        """Initialize provider(s). Must be called before first use."""
        await self._provider.initialize()
        if self._fallback:
            await self._fallback.initialize()
        self._initialized = True

    async def get_variant(
        self,
        experiment_id: str,
        user_id: str,
        properties: dict[str, Any] | None = None,
    ) -> str | None:
        """Return the variant name, or None if not assigned."""
        assignment = await self.get_assignment(experiment_id, user_id, properties)
        return assignment.variant_name if assignment else None

    async def get_assignment(
        self,
        experiment_id: str,
        user_id: str,
        properties: dict[str, Any] | None = None,
    ) -> Assignment | None:
        """Return the full Assignment, with fallback on error."""
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
        """Shut down provider(s) and release resources."""
        await self._provider.close()
        if self._fallback:
            await self._fallback.close()
        self._initialized = False
