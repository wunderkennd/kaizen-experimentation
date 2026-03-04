package com.experimentation.sdk

import com.google.common.truth.Truth.assertThat
import kotlinx.coroutines.test.runTest
import org.junit.Test

class MockProviderTest {
    @Test
    fun `getAssignment returns configured variant`() = runTest {
        val provider = MockProvider(
            assignments = mapOf(
                "exp_home" to Assignment(experimentId = "exp_home", variantName = "treatment"),
            )
        )
        provider.initialize()

        val attrs = UserAttributes(userId = "user-1")
        val assignment = provider.getAssignment("exp_home", attrs)

        assertThat(assignment).isNotNull()
        assertThat(assignment!!.variantName).isEqualTo("treatment")
    }

    @Test
    fun `getAssignment returns null for missing experiment`() = runTest {
        val provider = MockProvider()
        provider.initialize()

        val attrs = UserAttributes(userId = "user-1")
        val assignment = provider.getAssignment("nonexistent", attrs)

        assertThat(assignment).isNull()
    }

    @Test
    fun `setAssignment overrides at runtime`() = runTest {
        val provider = MockProvider()
        provider.initialize()

        provider.setAssignment("exp_recs", "v2")
        val attrs = UserAttributes(userId = "user-1")
        val assignment = provider.getAssignment("exp_recs", attrs)

        assertThat(assignment?.variantName).isEqualTo("v2")
    }

    @Test
    fun `client with mock provider`() = runTest {
        val provider = MockProvider(
            assignments = mapOf(
                "exp_recs" to Assignment(experimentId = "exp_recs", variantName = "treatment"),
            )
        )
        val client = ExperimentClient(provider = provider)
        client.initialize()

        assertThat(client.getVariant("exp_recs", "user-42")).isEqualTo("treatment")
        assertThat(client.getVariant("missing", "user-42")).isNull()

        client.close()
    }
}
