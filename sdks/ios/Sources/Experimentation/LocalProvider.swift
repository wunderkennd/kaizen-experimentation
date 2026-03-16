//
// LocalProvider.swift
// Experimentation Platform — iOS SDK
//
// Evaluates assignments locally using cached experiment config and UniFFI
// hash bindings. No network call required — ideal for offline / cold-start.
//
// The variant selection algorithm matches the Rust service exactly:
//   relative_bucket = bucket - start
//   cumulative = 0
//   for variant: cumulative += fraction * alloc_size; if relative < cumulative → return
//   fallthrough → last variant
//

import Foundation

// MARK: - Hash Provider Protocol

/// Abstracts hash computation so tests can inject a mock without native libs.
public protocol HashProvider: Sendable {
    func bucket(userId: String, salt: String, totalBuckets: UInt32) -> UInt32
    func isInAllocation(bucket: UInt32, startBucket: UInt32, endBucket: UInt32) -> Bool
}

// MARK: - UniFFI Hash Provider

#if canImport(experimentation_hashFFI)
/// Production hash provider that delegates to the generated UniFFI bindings.
/// Requires the UniFFI-generated `experimentation_hashFFI` module to be linked.
public struct UniFFIHashProvider: HashProvider {
    public init() {}

    public func bucket(userId: String, salt: String, totalBuckets: UInt32) -> UInt32 {
        uniffi_bucket(userId: userId, salt: salt, totalBuckets: totalBuckets)
    }

    public func isInAllocation(bucket: UInt32, startBucket: UInt32, endBucket: UInt32) -> Bool {
        uniffi_is_in_allocation(b: bucket, startBucket: startBucket, endBucket: endBucket)
    }
}
#endif

// MARK: - Config Types

/// Configuration for a single experiment (matches Rust ExperimentConfig).
public struct LocalExperimentConfig: Sendable {
    public let experimentId: String
    public let hashSalt: String
    public let layerId: String
    public let variants: [LocalVariantConfig]
    public let allocationStart: UInt32
    public let allocationEnd: UInt32
    public let totalBuckets: UInt32

    public init(
        experimentId: String,
        hashSalt: String,
        layerId: String,
        variants: [LocalVariantConfig],
        allocationStart: UInt32,
        allocationEnd: UInt32,
        totalBuckets: UInt32 = 10_000
    ) {
        self.experimentId = experimentId
        self.hashSalt = hashSalt
        self.layerId = layerId
        self.variants = variants
        self.allocationStart = allocationStart
        self.allocationEnd = allocationEnd
        self.totalBuckets = totalBuckets
    }
}

/// Configuration for a single variant within an experiment.
public struct LocalVariantConfig: Sendable {
    public let name: String
    public let trafficFraction: Double
    public let isControl: Bool
    public let payload: [String: String]

    public init(name: String, trafficFraction: Double, isControl: Bool = false, payload: [String: String] = [:]) {
        self.name = name
        self.trafficFraction = trafficFraction
        self.isControl = isControl
        self.payload = payload
    }
}

// MARK: - LocalProvider

/// Evaluates assignments locally using hash-based bucketing.
///
/// All assignments are returned with `fromCache: true` since they are computed
/// client-side without a server round-trip.
public final class LocalProvider: AssignmentProvider, @unchecked Sendable {
    private let hashProvider: HashProvider
    private let configs: [String: LocalExperimentConfig]

    public init(hashProvider: HashProvider, experiments: [LocalExperimentConfig]) {
        self.hashProvider = hashProvider
        var map: [String: LocalExperimentConfig] = [:]
        for exp in experiments {
            map[exp.experimentId] = exp
        }
        self.configs = map
    }

    public func initialize() async throws {
        // No initialization needed for local provider.
    }

    public func getAssignment(experimentId: String, attributes: UserAttributes) async throws -> Assignment? {
        guard let config = configs[experimentId] else {
            return nil
        }

        let bucket = hashProvider.bucket(
            userId: attributes.userId,
            salt: config.hashSalt,
            totalBuckets: config.totalBuckets
        )

        guard hashProvider.isInAllocation(
            bucket: bucket,
            startBucket: config.allocationStart,
            endBucket: config.allocationEnd
        ) else {
            return nil
        }

        guard let variant = selectVariant(config: config, bucket: bucket) else {
            return nil
        }

        return Assignment(
            experimentId: experimentId,
            variantName: variant.name,
            payload: variant.payload,
            fromCache: true
        )
    }

    public func getAllAssignments(attributes: UserAttributes) async throws -> [String: Assignment] {
        var results: [String: Assignment] = [:]
        for experimentId in configs.keys {
            if let assignment = try await getAssignment(experimentId: experimentId, attributes: attributes) {
                results[experimentId] = assignment
            }
        }
        return results
    }

    public func close() async {
        // No resources to release.
    }

    // MARK: - Private

    /// Replicates the Rust select_variant algorithm exactly.
    private func selectVariant(config: LocalExperimentConfig, bucket: UInt32) -> LocalVariantConfig? {
        guard !config.variants.isEmpty else { return nil }

        let allocSize = Double(config.allocationEnd - config.allocationStart + 1)
        let relativeBucket = Double(bucket - config.allocationStart)

        var cumulative = 0.0
        for variant in config.variants {
            cumulative += variant.trafficFraction * allocSize
            if relativeBucket < cumulative {
                return variant
            }
        }

        // Fallthrough guard: assign to last variant (handles FP rounding edge cases).
        return config.variants.last
    }
}
