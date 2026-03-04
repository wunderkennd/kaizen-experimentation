/**
 * Experimentation Platform — Android SDK
 *
 * Implements the Provider Abstraction pattern (ADR-007) with three backends:
 *   - RemoteProvider: Calls the Assignment Service via ConnectRPC
 *   - LocalProvider:  Evaluates assignments locally using cached config
 *   - MockProvider:   Returns deterministic assignments for testing
 *
 * Usage:
 *   val client = ExperimentClient(
 *       provider = RemoteProvider(baseUrl = "https://assignment.example.com")
 *   )
 *   val variant = client.getVariant("homepage_recs_v2", userId = "user-123")
 */
package com.experimentation.sdk

import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/** A variant assignment for a single experiment. */
data class Assignment(
    val experimentId: String,
    val variantName: String,
    val payload: Map<String, Any> = emptyMap(),
    val fromCache: Boolean = false,
)

/** User attributes for targeting evaluation. */
data class UserAttributes(
    val userId: String,
    val properties: Map<String, Any> = emptyMap(),
)

/** Configuration for an experiment (used by LocalProvider). */
data class ExperimentConfig(
    val experimentId: String,
    val hashSalt: String,
    val layerName: String,
    val variants: List<VariantConfig>,
    val allocationStart: Int,
    val allocationEnd: Int,
    val totalBuckets: Int = 10_000,
)

data class VariantConfig(
    val name: String,
    val trafficFraction: Double,
    val isControl: Boolean = false,
    val payload: Map<String, Any> = emptyMap(),
)

// ---------------------------------------------------------------------------
// Provider Interface
// ---------------------------------------------------------------------------

/**
 * Provider abstraction — all assignment backends implement this interface.
 * See ADR-007 for the design rationale.
 */
interface AssignmentProvider {
    suspend fun initialize()
    suspend fun getAssignment(experimentId: String, attributes: UserAttributes): Assignment?
    suspend fun getAllAssignments(attributes: UserAttributes): Map<String, Assignment>
    suspend fun close()
}

// ---------------------------------------------------------------------------
// RemoteProvider
// ---------------------------------------------------------------------------

/** Calls the Assignment Service via ConnectRPC. */
class RemoteProvider(
    private val baseUrl: String,
    private val timeoutMs: Long = 2_000L,
) : AssignmentProvider {

    override suspend fun initialize() {
        // TODO (Agent-1): Create ConnectRPC client for AssignmentService
    }

    override suspend fun getAssignment(experimentId: String, attributes: UserAttributes): Assignment? {
        // TODO (Agent-1): Call AssignmentService.GetAssignment via ConnectRPC
        return null
    }

    override suspend fun getAllAssignments(attributes: UserAttributes): Map<String, Assignment> {
        // TODO (Agent-1): Call AssignmentService.GetAllAssignments
        return emptyMap()
    }

    override suspend fun close() {
        // TODO (Agent-1): Close transport
    }
}

// ---------------------------------------------------------------------------
// MockProvider
// ---------------------------------------------------------------------------

/** Returns deterministic assignments for testing. */
class MockProvider(
    assignments: Map<String, Assignment> = emptyMap(),
) : AssignmentProvider {

    private val mutex = Mutex()
    private val assignments = assignments.toMutableMap()

    override suspend fun initialize() {}

    override suspend fun getAssignment(experimentId: String, attributes: UserAttributes): Assignment? =
        mutex.withLock { assignments[experimentId] }

    override suspend fun getAllAssignments(attributes: UserAttributes): Map<String, Assignment> =
        mutex.withLock { assignments.toMap() }

    /** Override an assignment at runtime (useful in tests). */
    suspend fun setAssignment(experimentId: String, variantName: String) {
        mutex.withLock {
            assignments[experimentId] = Assignment(
                experimentId = experimentId,
                variantName = variantName,
            )
        }
    }

    override suspend fun close() {
        mutex.withLock { assignments.clear() }
    }
}

// ---------------------------------------------------------------------------
// ExperimentClient
// ---------------------------------------------------------------------------

/** Main entry point for the Android SDK. */
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

    /** Returns the variant name, or null if not assigned. */
    suspend fun getVariant(
        experimentId: String,
        userId: String,
        properties: Map<String, Any> = emptyMap(),
    ): String? {
        val assignment = getAssignment(experimentId, userId, properties)
        return assignment?.variantName
    }

    /** Returns the full Assignment with fallback on error. */
    suspend fun getAssignment(
        experimentId: String,
        userId: String,
        properties: Map<String, Any> = emptyMap(),
    ): Assignment? {
        if (!initialized) initialize()

        val attrs = UserAttributes(userId = userId, properties = properties)
        return try {
            provider.getAssignment(experimentId, attrs)
        } catch (e: Exception) {
            if (fallback != null) {
                fallback.getAssignment(experimentId, attrs)
            } else {
                throw e
            }
        }
    }

    suspend fun close() {
        provider.close()
        fallback?.close()
        initialized = false
    }
}
