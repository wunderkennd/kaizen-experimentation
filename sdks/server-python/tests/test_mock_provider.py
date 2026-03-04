"""Tests for MockProvider — validates the provider abstraction interface."""

import pytest

from experimentation import Assignment, ExperimentClient, MockProvider, UserAttributes


@pytest.fixture
def mock_provider() -> MockProvider:
    return MockProvider(
        assignments={
            "exp_homepage": Assignment(
                experiment_id="exp_homepage",
                variant_name="treatment",
                payload={"color": "blue"},
            ),
        }
    )


async def test_get_assignment(mock_provider: MockProvider) -> None:
    await mock_provider.initialize()
    attrs = UserAttributes(user_id="user-1")
    a = await mock_provider.get_assignment("exp_homepage", attrs)
    assert a is not None
    assert a.variant_name == "treatment"
    assert a.payload == {"color": "blue"}


async def test_get_assignment_missing(mock_provider: MockProvider) -> None:
    await mock_provider.initialize()
    attrs = UserAttributes(user_id="user-1")
    a = await mock_provider.get_assignment("nonexistent", attrs)
    assert a is None


async def test_get_all_assignments(mock_provider: MockProvider) -> None:
    await mock_provider.initialize()
    attrs = UserAttributes(user_id="user-1")
    all_a = await mock_provider.get_all_assignments(attrs)
    assert "exp_homepage" in all_a


async def test_set_assignment(mock_provider: MockProvider) -> None:
    await mock_provider.initialize()
    mock_provider.set_assignment("exp_checkout", "control")
    attrs = UserAttributes(user_id="user-1")
    a = await mock_provider.get_assignment("exp_checkout", attrs)
    assert a is not None
    assert a.variant_name == "control"


async def test_client_with_mock() -> None:
    provider = MockProvider(
        assignments={
            "exp_recs": Assignment(
                experiment_id="exp_recs",
                variant_name="v2",
            ),
        }
    )
    client = ExperimentClient(provider=provider)
    await client.initialize()

    variant = await client.get_variant("exp_recs", "user-42")
    assert variant == "v2"

    variant = await client.get_variant("missing", "user-42")
    assert variant is None

    await client.close()
