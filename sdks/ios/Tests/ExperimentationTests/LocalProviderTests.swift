import XCTest
@testable import Experimentation

/// Mock hash provider for testing LocalProvider without native UniFFI library.
struct MockHashProvider: HashProvider {
    /// Deterministic bucket: hash(userId + salt) mod totalBuckets.
    /// Uses a simple FNV-1a hash for test determinism.
    func bucket(userId: String, salt: String, totalBuckets: UInt32) -> UInt32 {
        let key = "\(userId)\0\(salt)"
        var hash: UInt32 = 0x811c9dc5
        for byte in key.utf8 {
            hash ^= UInt32(byte)
            hash &*= 0x01000193
        }
        return hash % totalBuckets
    }

    func isInAllocation(bucket: UInt32, startBucket: UInt32, endBucket: UInt32) -> Bool {
        bucket >= startBucket && bucket <= endBucket
    }
}

final class LocalProviderTests: XCTestCase {
    private let hashProvider = MockHashProvider()

    private func makeExperiment(
        id: String = "exp_1",
        salt: String = "salt_1",
        variants: [LocalVariantConfig] = [
            LocalVariantConfig(name: "control", trafficFraction: 0.5, isControl: true),
            LocalVariantConfig(name: "treatment", trafficFraction: 0.5)
        ],
        allocationStart: UInt32 = 0,
        allocationEnd: UInt32 = 9999
    ) -> LocalExperimentConfig {
        LocalExperimentConfig(
            experimentId: id,
            hashSalt: salt,
            layerId: "layer_1",
            variants: variants,
            allocationStart: allocationStart,
            allocationEnd: allocationEnd,
            totalBuckets: 10_000
        )
    }

    // MARK: - Deterministic assignment

    func testDeterministicAssignment() async throws {
        let exp = makeExperiment()
        let provider = LocalProvider(hashProvider: hashProvider, experiments: [exp])
        let attrs = UserAttributes(userId: "user_42")

        let a1 = try await provider.getAssignment(experimentId: "exp_1", attributes: attrs)
        let a2 = try await provider.getAssignment(experimentId: "exp_1", attributes: attrs)

        XCTAssertNotNil(a1)
        XCTAssertEqual(a1, a2, "Same user + salt must always get the same assignment")
    }

    // MARK: - Out of allocation returns nil

    func testOutOfAllocationReturnsNil() async throws {
        // Allocation covers only buckets 0–0 (1 bucket out of 10000).
        // Most users will be outside this range.
        let exp = makeExperiment(allocationStart: 0, allocationEnd: 0)
        let provider = LocalProvider(hashProvider: hashProvider, experiments: [exp])

        // Try many users — most should be nil.
        var nilCount = 0
        for i in 0..<100 {
            let attrs = UserAttributes(userId: "out_of_alloc_user_\(i)")
            let result = try await provider.getAssignment(experimentId: "exp_1", attributes: attrs)
            if result == nil { nilCount += 1 }
        }
        XCTAssertGreaterThan(nilCount, 90, "Most users should be outside 1-bucket allocation")
    }

    // MARK: - Unknown experiment returns nil

    func testUnknownExperimentReturnsNil() async throws {
        let exp = makeExperiment()
        let provider = LocalProvider(hashProvider: hashProvider, experiments: [exp])
        let attrs = UserAttributes(userId: "user_1")

        let result = try await provider.getAssignment(experimentId: "nonexistent", attributes: attrs)
        XCTAssertNil(result)
    }

    // MARK: - getAllAssignments

    func testGetAllAssignments() async throws {
        let exp1 = makeExperiment(id: "exp_a", salt: "salt_a")
        let exp2 = makeExperiment(id: "exp_b", salt: "salt_b")
        let provider = LocalProvider(hashProvider: hashProvider, experiments: [exp1, exp2])
        let attrs = UserAttributes(userId: "user_1")

        let all = try await provider.getAllAssignments(attributes: attrs)
        XCTAssertEqual(all.count, 2)
        XCTAssertNotNil(all["exp_a"])
        XCTAssertNotNil(all["exp_b"])
    }

    // MARK: - Cumulative fraction boundary

    func testCumulativeFractionBoundary() async throws {
        // 3 variants: 10%, 80%, 10%.
        let variants = [
            LocalVariantConfig(name: "v1", trafficFraction: 0.1, isControl: true),
            LocalVariantConfig(name: "v2", trafficFraction: 0.8),
            LocalVariantConfig(name: "v3", trafficFraction: 0.1),
        ]
        let exp = makeExperiment(variants: variants)
        let provider = LocalProvider(hashProvider: hashProvider, experiments: [exp])

        // Run many users and verify the middle variant gets ~80% of traffic.
        var counts: [String: Int] = [:]
        for i in 0..<1000 {
            let attrs = UserAttributes(userId: "fraction_user_\(i)")
            if let assignment = try await provider.getAssignment(experimentId: "exp_1", attributes: attrs) {
                counts[assignment.variantName, default: 0] += 1
            }
        }

        let v2Count = counts["v2"] ?? 0
        let v2Fraction = Double(v2Count) / 1000.0
        XCTAssertGreaterThan(v2Fraction, 0.65, "v2 (80% traffic) should get majority: got \(v2Fraction)")
        XCTAssertLessThan(v2Fraction, 0.95, "v2 should not get everything: got \(v2Fraction)")
    }

    // MARK: - fromCache flag

    func testFromCacheFlag() async throws {
        let exp = makeExperiment()
        let provider = LocalProvider(hashProvider: hashProvider, experiments: [exp])
        let attrs = UserAttributes(userId: "user_1")

        let assignment = try await provider.getAssignment(experimentId: "exp_1", attributes: attrs)
        XCTAssertNotNil(assignment)
        XCTAssertTrue(assignment!.fromCache, "LocalProvider assignments must have fromCache=true")
    }
}
