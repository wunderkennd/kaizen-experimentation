/**
 * LocalProvider — Android SDK
 *
 * Evaluates assignments locally using cached experiment config and UniFFI
 * hash bindings. No network call required — ideal for offline / cold-start.
 *
 * The variant selection algorithm matches the Rust service exactly:
 *   relative_bucket = bucket - start
 *   cumulative = 0
 *   for variant: cumulative += fraction * alloc_size; if relative < cumulative → return
 *   fallthrough → last variant
 */
package com.experimentation.sdk

// ---------------------------------------------------------------------------
// Hash Provider Interface
// ---------------------------------------------------------------------------

/**
 * Abstracts hash computation so tests can inject a mock without native libs.
 */
interface HashProvider {
    fun bucket(userId: String, salt: String, totalBuckets: Int): Int
    fun isInAllocation(bucket: Int, startBucket: Int, endBucket: Int): Boolean
}

// ---------------------------------------------------------------------------
// LocalProvider
// ---------------------------------------------------------------------------

/**
 * Evaluates assignments locally using hash-based bucketing.
 *
 * All assignments are returned with `fromCache = true` since they are computed
 * client-side without a server round-trip.
 */
class LocalProvider(
    private val hashProvider: HashProvider,
    experiments: List<ExperimentConfig>,
) : AssignmentProvider {

    private val configs: Map<String, ExperimentConfig> =
        experiments.associateBy { it.experimentId }

    override suspend fun initialize() {
        // No initialization needed for local provider.
    }

    override suspend fun getAssignment(
        experimentId: String,
        attributes: UserAttributes,
    ): Assignment? {
        val config = configs[experimentId] ?: return null
        val userId = attributes.userId

        val bucket = hashProvider.bucket(userId, config.hashSalt, config.totalBuckets)

        if (!hashProvider.isInAllocation(bucket, config.allocationStart, config.allocationEnd)) {
            return null
        }

        val variant = selectVariant(config, bucket) ?: return null

        return Assignment(
            experimentId = experimentId,
            variantName = variant.name,
            payload = variant.payload,
            fromCache = true,
        )
    }

    override suspend fun getAllAssignments(
        attributes: UserAttributes,
    ): Map<String, Assignment> {
        val results = mutableMapOf<String, Assignment>()
        for (experimentId in configs.keys) {
            val assignment = getAssignment(experimentId, attributes)
            if (assignment != null) {
                results[experimentId] = assignment
            }
        }
        return results
    }

    override suspend fun close() {
        // No resources to release.
    }

    // -- Private --

    /** Replicates the Rust select_variant algorithm exactly. */
    private fun selectVariant(config: ExperimentConfig, bucket: Int): VariantConfig? {
        if (config.variants.isEmpty()) return null

        val allocSize = (config.allocationEnd - config.allocationStart + 1).toDouble()
        val relativeBucket = (bucket - config.allocationStart).toDouble()

        var cumulative = 0.0
        for (variant in config.variants) {
            cumulative += variant.trafficFraction * allocSize
            if (relativeBucket < cumulative) {
                return variant
            }
        }

        // Fallthrough guard: assign to last variant (handles FP rounding edge cases).
        return config.variants.last()
    }
}
