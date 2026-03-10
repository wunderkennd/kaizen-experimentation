package com.experimentation.sdk

import kotlinx.coroutines.test.runTest
import org.junit.Assert.*
import org.junit.Test

/**
 * Mock hash provider for testing LocalProvider without native UniFFI library.
 * Uses FNV-1a for deterministic hashing.
 */
class MockHashProvider : HashProvider {
    override fun bucket(userId: String, salt: String, totalBuckets: Int): Int {
        val key = "$userId\u0000$salt"
        var hash = 0x811c9dc5.toInt()
        for (byte in key.toByteArray(Charsets.UTF_8)) {
            hash = hash xor byte.toInt()
            hash = (hash.toLong() * 0x01000193L).toInt()
        }
        return (hash.toLong() and 0xFFFFFFFFL).toInt() % totalBuckets
    }

    override fun isInAllocation(bucket: Int, startBucket: Int, endBucket: Int): Boolean =
        bucket in startBucket..endBucket
}

class LocalProviderTest {
    private val hashProvider = MockHashProvider()

    private fun makeExperiment(
        id: String = "exp_1",
        salt: String = "salt_1",
        variants: List<VariantConfig> = listOf(
            VariantConfig(name = "control", trafficFraction = 0.5, isControl = true),
            VariantConfig(name = "treatment", trafficFraction = 0.5),
        ),
        allocationStart: Int = 0,
        allocationEnd: Int = 9999,
    ) = ExperimentConfig(
        experimentId = id,
        hashSalt = salt,
        layerName = "layer_1",
        variants = variants,
        allocationStart = allocationStart,
        allocationEnd = allocationEnd,
        totalBuckets = 10_000,
    )

    @Test
    fun deterministicAssignment() = runTest {
        val exp = makeExperiment()
        val provider = LocalProvider(hashProvider, listOf(exp))
        val attrs = UserAttributes(userId = "user_42")

        val a1 = provider.getAssignment("exp_1", attrs)
        val a2 = provider.getAssignment("exp_1", attrs)

        assertNotNull(a1)
        assertEquals("Same user + salt must always get the same assignment", a1, a2)
    }

    @Test
    fun outOfAllocationReturnsNull() = runTest {
        val exp = makeExperiment(allocationStart = 0, allocationEnd = 0)
        val provider = LocalProvider(hashProvider, listOf(exp))

        var nullCount = 0
        for (i in 0 until 100) {
            val attrs = UserAttributes(userId = "out_of_alloc_user_$i")
            if (provider.getAssignment("exp_1", attrs) == null) nullCount++
        }
        assertTrue("Most users should be outside 1-bucket allocation, got $nullCount nulls", nullCount > 90)
    }

    @Test
    fun unknownExperimentReturnsNull() = runTest {
        val exp = makeExperiment()
        val provider = LocalProvider(hashProvider, listOf(exp))
        val attrs = UserAttributes(userId = "user_1")

        assertNull(provider.getAssignment("nonexistent", attrs))
    }

    @Test
    fun getAllAssignments() = runTest {
        val exp1 = makeExperiment(id = "exp_a", salt = "salt_a")
        val exp2 = makeExperiment(id = "exp_b", salt = "salt_b")
        val provider = LocalProvider(hashProvider, listOf(exp1, exp2))
        val attrs = UserAttributes(userId = "user_1")

        val all = provider.getAllAssignments(attrs)
        assertEquals(2, all.size)
        assertNotNull(all["exp_a"])
        assertNotNull(all["exp_b"])
    }

    @Test
    fun cumulativeFractionBoundary() = runTest {
        val variants = listOf(
            VariantConfig(name = "v1", trafficFraction = 0.1, isControl = true),
            VariantConfig(name = "v2", trafficFraction = 0.8),
            VariantConfig(name = "v3", trafficFraction = 0.1),
        )
        val exp = makeExperiment(variants = variants)
        val provider = LocalProvider(hashProvider, listOf(exp))

        val counts = mutableMapOf<String, Int>()
        for (i in 0 until 1000) {
            val attrs = UserAttributes(userId = "fraction_user_$i")
            val assignment = provider.getAssignment("exp_1", attrs)
            if (assignment != null) {
                counts[assignment.variantName] = (counts[assignment.variantName] ?: 0) + 1
            }
        }

        val v2Count = counts["v2"] ?: 0
        val v2Fraction = v2Count / 1000.0
        assertTrue("v2 (80% traffic) should get majority: got $v2Fraction", v2Fraction > 0.65)
        assertTrue("v2 should not get everything: got $v2Fraction", v2Fraction < 0.95)
    }

    @Test
    fun fromCacheFlag() = runTest {
        val exp = makeExperiment()
        val provider = LocalProvider(hashProvider, listOf(exp))
        val attrs = UserAttributes(userId = "user_1")

        val assignment = provider.getAssignment("exp_1", attrs)
        assertNotNull(assignment)
        assertTrue("LocalProvider assignments must have fromCache=true", assignment!!.fromCache)
    }
}
