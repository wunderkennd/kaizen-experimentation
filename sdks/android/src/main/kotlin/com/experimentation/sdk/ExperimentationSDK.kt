/**
 * Experimentation Platform — Android SDK
 *
 * Implements the Provider Abstraction pattern (ADR-007) for Kotlin/Android.
 *
 * Usage:
 *   val client = ExperimentClient(provider = RemoteProvider(baseUrl = "https://..."))
 *   val variant = client.getVariant("homepage_recs_v2", userId = "user-123")
 */
package com.experimentation.sdk

import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

// ---------------------------------------------------------------------------
// Core Types
// ---------------------------------------------------------------------------

data class Assignment(
    val experimentId: String,
    val variantName: String,
    val payload: Map<String, Any> = emptyMap(),
    val fromCache: Boolean = false,
)

data class UserAttributes(
    val userId: String,
    val properties: Map<String, Any> = emptyMap(),
)

// ---------------------------------------------------------------------------
// Provider Interface
// ---------------------------------------------------------------------------

/**
 * All assignment backends implement this interface (ADR-007).
 */
interface AssignmentProvider {
    suspend fun initialize()
    suspend fun getAssignment(experimentId: String, attrs: UserAttributes): Assignment?
    suspend fun getAllAssignments(attrs: UserAttributes): Map<String, Assignment>
    suspend fun close()
}

// ---------------------------------------------------------------------------
// RemoteProvider
// ---------------------------------------------------------------------------

/**
 * Calls the Assignment Service via ConnectRPC.
 */
class RemoteProvider(
    private val baseUrl: String,
    private val timeoutMs: Long = 2000L,
) : AssignmentProvider {

    override suspend fun initialize() {
        // TODO (Agent-1): Create ConnectRPC-Kotlin transport
    }

    override suspend fun getAssignment(experimentId: String, attrs: UserAttributes): Assignment? {
        // TODO (Agent-1): Call AssignmentService.GetAssignment
        return null
    }

    override suspend fun getAllAssignments(attrs: UserAttributes): Map<String, Assignment> {
        // TODO (Agent-1): Call AssignmentService.GetAllAssignments
        return emptyMap()
    }

    override suspend fun close() {}
}

// ---------------------------------------------------------------------------
// MockProvider
// ---------------------------------------------------------------------------

/**
 * Returns deterministic assignments for testing.
 */
class MockProvider(
    assignments: Map<String, String> = emptyMap(),
) : AssignmentProvider {

    private val mutex = Mutex()
    private val assignments = assignments.mapValues { (expId, variant) ->
        Assignment(experimentId = expId, variantName = variant)
    }.toMutableMap()

    override suspend fun initialize() {}

    override suspend fun getAssignment(experimentId: String, attrs: UserAttributes): Assignment? =
        mutex.withLock { assignments[experimentId] }

    override suspend fun getAllAssignments(attrs: UserAttributes): Map<String, Assignment> =
        mutex.withLock { assignments.toMap() }

    fun setAssignment(experimentId: String, variantName: String) {
        assignments[experimentId] = Assignment(experimentId = experimentId, variantName = variantName)
    }

    override suspend fun close() = mutex.withLock { assignments.clear() }
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

class ExperimentClient(
    private val provider: AssignmentProvider,
    private val fallback: AssignmentProvider? = null,
) {
    private var initialized = false

    suspend fun initialize() {
        provider.initialize()
        fallback?.initialize()
        initialized = true
    }

    suspend fun getVariant(
        experimentId: String,
        userId: String,
        properties: Map<String, Any> = emptyMap(),
    ): String? {
        if (!initialized) initialize()
        val attrs = UserAttributes(userId = userId, properties = properties)
        return try {
            provider.getAssignment(experimentId, attrs)?.variantName
        } catch (e: Exception) {
            fallback?.getAssignment(experimentId, attrs)?.variantName
        }
    }

    suspend fun close() {
        provider.close()
        fallback?.close()
        initialized = false
    }
}
