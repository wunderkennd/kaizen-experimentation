"""Tests for LocalProvider hash-based variant assignment."""

import pytest

from experimentation import (
    Assignment,
    ExperimentConfig,
    LocalProvider,
    UserAttributes,
    VariantConfig,
)


# ---------------------------------------------------------------------------
# Hash parity with test vectors (from test-vectors/hash_vectors.json)
# ---------------------------------------------------------------------------


def _compute_bucket(user_id: str, salt: str, total_buckets: int) -> int:
    import mmh3

    key = f"{user_id}\x00{salt}"
    raw_hash = mmh3.hash(key, seed=0, signed=False)
    return raw_hash % total_buckets


PARITY_VECTORS = [
    ("user_000000", "experiment_default_salt", 10000, 3913),
    ("user_000001", "experiment_default_salt", 10000, 4234),
    ("user_000002", "experiment_default_salt", 10000, 5578),
    ("user_000003", "experiment_default_salt", 10000, 8009),
    ("user_000004", "experiment_default_salt", 10000, 2419),
    ("user_000005", "experiment_default_salt", 10000, 5885),
    ("user_000006", "experiment_default_salt", 10000, 5586),
    ("user_000007", "experiment_default_salt", 10000, 9853),
    ("user_000008", "experiment_default_salt", 10000, 2730),
    ("user_000009", "experiment_default_salt", 10000, 27),
]


@pytest.mark.parametrize("user_id,salt,total_buckets,expected", PARITY_VECTORS)
def test_bucket_parity(user_id: str, salt: str, total_buckets: int, expected: int) -> None:
    assert _compute_bucket(user_id, salt, total_buckets) == expected


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

TWO_VARIANT_CONFIG = ExperimentConfig(
    experiment_id="exp_ab_test",
    hash_salt="salt_ab",
    layer_name="default",
    variants=[
        VariantConfig(name="control", traffic_fraction=0.5, is_control=True, payload={"color": "blue"}),
        VariantConfig(name="treatment", traffic_fraction=0.5, is_control=False, payload={"color": "red"}),
    ],
    allocation_start=0,
    allocation_end=9999,
    total_buckets=10000,
)

THREE_VARIANT_CONFIG = ExperimentConfig(
    experiment_id="exp_abc",
    hash_salt="salt_abc",
    layer_name="default",
    variants=[
        VariantConfig(name="control", traffic_fraction=0.34, is_control=True),
        VariantConfig(name="variant_a", traffic_fraction=0.33),
        VariantConfig(name="variant_b", traffic_fraction=0.33),
    ],
    allocation_start=0,
    allocation_end=9999,
    total_buckets=10000,
)


# ---------------------------------------------------------------------------
# LocalProvider tests
# ---------------------------------------------------------------------------


@pytest.mark.asyncio
async def test_unknown_experiment() -> None:
    provider = LocalProvider([TWO_VARIANT_CONFIG])
    result = await provider.get_assignment("nonexistent", UserAttributes(user_id="user1"))
    assert result is None


@pytest.mark.asyncio
async def test_deterministic() -> None:
    provider = LocalProvider([TWO_VARIANT_CONFIG])
    attrs = UserAttributes(user_id="user_stable_123")
    a1 = await provider.get_assignment("exp_ab_test", attrs)
    a2 = await provider.get_assignment("exp_ab_test", attrs)
    assert a1 is not None
    assert a2 is not None
    assert a1.variant_name == a2.variant_name


@pytest.mark.asyncio
async def test_from_cache() -> None:
    provider = LocalProvider([TWO_VARIANT_CONFIG])
    result = await provider.get_assignment("exp_ab_test", UserAttributes(user_id="user1"))
    assert result is not None
    assert result.from_cache is True


@pytest.mark.asyncio
async def test_payload() -> None:
    provider = LocalProvider([TWO_VARIANT_CONFIG])
    result = await provider.get_assignment("exp_ab_test", UserAttributes(user_id="user1"))
    assert result is not None
    assert result.payload is not None


@pytest.mark.asyncio
async def test_exclusion() -> None:
    narrow = ExperimentConfig(
        experiment_id="exp_narrow",
        hash_salt="salt_ab",
        layer_name="default",
        variants=TWO_VARIANT_CONFIG.variants,
        allocation_start=0,
        allocation_end=0,  # only bucket 0
        total_buckets=10000,
    )
    provider = LocalProvider([narrow])
    null_count = 0
    for i in range(50):
        result = await provider.get_assignment(
            "exp_narrow", UserAttributes(user_id=f"exclude_test_{i}")
        )
        if result is None:
            null_count += 1
    assert null_count > 40


@pytest.mark.asyncio
async def test_distribution() -> None:
    provider = LocalProvider([TWO_VARIANT_CONFIG])
    counts: dict[str, int] = {"control": 0, "treatment": 0}
    for i in range(1000):
        result = await provider.get_assignment(
            "exp_ab_test", UserAttributes(user_id=f"dist_user_{i}")
        )
        if result is not None:
            counts[result.variant_name] += 1
    assert counts["control"] > 350
    assert counts["treatment"] > 350


@pytest.mark.asyncio
async def test_three_variants() -> None:
    provider = LocalProvider([THREE_VARIANT_CONFIG])
    variants: set[str] = set()
    for i in range(500):
        result = await provider.get_assignment(
            "exp_abc", UserAttributes(user_id=f"three_var_{i}")
        )
        if result is not None:
            variants.add(result.variant_name)
    assert len(variants) == 3


@pytest.mark.asyncio
async def test_fp_rounding_fallback() -> None:
    fp_config = ExperimentConfig(
        experiment_id="exp_fp",
        hash_salt="salt_fp",
        layer_name="default",
        variants=[
            VariantConfig(name="a", traffic_fraction=0.333, is_control=True),
            VariantConfig(name="b", traffic_fraction=0.333),
            VariantConfig(name="c", traffic_fraction=0.334),
        ],
        allocation_start=0,
        allocation_end=9999,
        total_buckets=10000,
    )
    provider = LocalProvider([fp_config])
    valid = {"a", "b", "c"}
    for i in range(100):
        result = await provider.get_assignment(
            "exp_fp", UserAttributes(user_id=f"fp_user_{i}")
        )
        assert result is not None
        assert result.variant_name in valid


@pytest.mark.asyncio
async def test_get_all_assignments() -> None:
    provider = LocalProvider([TWO_VARIANT_CONFIG, THREE_VARIANT_CONFIG])
    results = await provider.get_all_assignments(UserAttributes(user_id="multi_user_1"))
    assert len(results) == 2
    assert "exp_ab_test" in results
    assert "exp_abc" in results
