"""Tests for RemoteProvider JSON HTTP transport."""

import json

import httpx
import pytest

from experimentation import (
    ExperimentClient,
    MockProvider,
    RemoteProvider,
    UserAttributes,
)


# ---------------------------------------------------------------------------
# Mock transport
# ---------------------------------------------------------------------------


def _mock_transport(handler):
    """Create an httpx.MockTransport from a handler function."""
    return httpx.MockTransport(handler)


def _ok_assignment_handler(request: httpx.Request) -> httpx.Response:
    """Returns a successful assignment response."""
    body = json.loads(request.content)
    return httpx.Response(
        200,
        json={
            "experimentId": body.get("experimentId", "exp1"),
            "variantId": "treatment",
            "payloadJson": '{"color":"red"}',
            "assignmentProbability": 0.5,
            "isActive": True,
        },
    )


def _not_found_handler(request: httpx.Request) -> httpx.Response:
    return httpx.Response(404, json={"code": 404, "message": "not found"})


def _inactive_handler(request: httpx.Request) -> httpx.Response:
    return httpx.Response(
        200,
        json={
            "experimentId": "exp1",
            "variantId": "",
            "isActive": False,
        },
    )


def _bulk_handler(request: httpx.Request) -> httpx.Response:
    return httpx.Response(
        200,
        json={
            "assignments": [
                {"experimentId": "exp1", "variantId": "control", "payloadJson": "{}", "isActive": True},
                {"experimentId": "exp2", "variantId": "treatment", "payloadJson": '{"x":1}', "isActive": True},
                {"experimentId": "exp3", "variantId": "", "isActive": False},
            ]
        },
    )


def _error_handler(request: httpx.Request) -> httpx.Response:
    return httpx.Response(500, json={"code": 500, "message": "internal"})


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------


@pytest.mark.asyncio
async def test_get_assignment_success() -> None:
    provider = RemoteProvider(base_url="http://localhost:8080")
    await provider.initialize()
    # Swap the internal client's transport
    provider._client = httpx.AsyncClient(
        base_url="http://localhost:8080",
        transport=_mock_transport(_ok_assignment_handler),
    )

    result = await provider.get_assignment("exp1", UserAttributes(user_id="user-1"))
    assert result is not None
    assert result.experiment_id == "exp1"
    assert result.variant_name == "treatment"
    assert result.payload == {"color": "red"}
    assert result.from_cache is False

    await provider.close()


@pytest.mark.asyncio
async def test_get_assignment_not_found() -> None:
    provider = RemoteProvider(base_url="http://localhost:8080")
    await provider.initialize()
    provider._client = httpx.AsyncClient(
        base_url="http://localhost:8080",
        transport=_mock_transport(_not_found_handler),
    )

    result = await provider.get_assignment("missing", UserAttributes(user_id="user-1"))
    assert result is None

    await provider.close()


@pytest.mark.asyncio
async def test_get_assignment_inactive() -> None:
    provider = RemoteProvider(base_url="http://localhost:8080")
    await provider.initialize()
    provider._client = httpx.AsyncClient(
        base_url="http://localhost:8080",
        transport=_mock_transport(_inactive_handler),
    )

    result = await provider.get_assignment("exp1", UserAttributes(user_id="user-1"))
    assert result is None

    await provider.close()


@pytest.mark.asyncio
async def test_get_assignment_server_error() -> None:
    provider = RemoteProvider(base_url="http://localhost:8080")
    await provider.initialize()
    provider._client = httpx.AsyncClient(
        base_url="http://localhost:8080",
        transport=_mock_transport(_error_handler),
    )

    result = await provider.get_assignment("exp1", UserAttributes(user_id="user-1"))
    assert result is None

    await provider.close()


@pytest.mark.asyncio
async def test_get_assignment_not_initialized() -> None:
    provider = RemoteProvider(base_url="http://localhost:8080")
    with pytest.raises(RuntimeError, match="not initialized"):
        await provider.get_assignment("exp1", UserAttributes(user_id="user-1"))


@pytest.mark.asyncio
async def test_get_assignment_empty_payload() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(
            200,
            json={
                "experimentId": "exp1",
                "variantId": "control",
                "payloadJson": "",
                "isActive": True,
            },
        )

    provider = RemoteProvider(base_url="http://localhost:8080")
    await provider.initialize()
    provider._client = httpx.AsyncClient(
        base_url="http://localhost:8080",
        transport=_mock_transport(handler),
    )

    result = await provider.get_assignment("exp1", UserAttributes(user_id="user-1"))
    assert result is not None
    assert result.payload == {}

    await provider.close()


@pytest.mark.asyncio
async def test_get_assignment_sends_attributes() -> None:
    captured = {}

    def handler(request: httpx.Request) -> httpx.Response:
        captured.update(json.loads(request.content))
        return httpx.Response(
            200,
            json={"experimentId": "exp1", "variantId": "v", "isActive": True},
        )

    provider = RemoteProvider(base_url="http://localhost:8080")
    await provider.initialize()
    provider._client = httpx.AsyncClient(
        base_url="http://localhost:8080",
        transport=_mock_transport(handler),
    )

    await provider.get_assignment(
        "exp1",
        UserAttributes(user_id="user-1", properties={"plan": "premium", "age": 30}),
    )

    assert captured["attributes"]["plan"] == "premium"
    assert captured["attributes"]["age"] == "30"

    await provider.close()


@pytest.mark.asyncio
async def test_get_all_assignments() -> None:
    provider = RemoteProvider(base_url="http://localhost:8080")
    await provider.initialize()
    provider._client = httpx.AsyncClient(
        base_url="http://localhost:8080",
        transport=_mock_transport(_bulk_handler),
    )

    results = await provider.get_all_assignments(UserAttributes(user_id="user-1"))
    assert len(results) == 2
    assert results["exp1"].variant_name == "control"
    assert results["exp2"].variant_name == "treatment"
    assert "exp3" not in results  # inactive

    await provider.close()


@pytest.mark.asyncio
async def test_get_all_assignments_error() -> None:
    provider = RemoteProvider(base_url="http://localhost:8080")
    await provider.initialize()
    provider._client = httpx.AsyncClient(
        base_url="http://localhost:8080",
        transport=_mock_transport(_error_handler),
    )

    results = await provider.get_all_assignments(UserAttributes(user_id="user-1"))
    assert results == {}

    await provider.close()


@pytest.mark.asyncio
async def test_close_cleanup() -> None:
    provider = RemoteProvider(base_url="http://localhost:8080")
    await provider.initialize()
    assert provider._client is not None
    await provider.close()
    assert provider._client is None


@pytest.mark.asyncio
async def test_close_idempotent() -> None:
    provider = RemoteProvider(base_url="http://localhost:8080")
    await provider.initialize()
    await provider.close()
    await provider.close()  # should not raise


# ---------------------------------------------------------------------------
# Client fallback chain
# ---------------------------------------------------------------------------


@pytest.mark.asyncio
async def test_client_fallback_on_error() -> None:
    """When RemoteProvider raises, client falls back to MockProvider."""
    remote = RemoteProvider(base_url="http://localhost:8080")
    # Don't initialize — will raise RuntimeError

    mock = MockProvider({"exp1": "fallback-variant"})

    client = ExperimentClient(provider=remote, fallback_provider=mock)
    # Initialize will fail for remote but that's OK — get_assignment will raise
    # and trigger fallback
    await mock.initialize()
    client._initialized = True  # skip auto-init which would fail on remote
    client._provider = remote

    result = await client.get_assignment("exp1", "user-1")
    assert result is not None
    assert result.variant_name == "fallback-variant"

    await client.close()
