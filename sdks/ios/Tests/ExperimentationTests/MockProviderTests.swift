import XCTest
@testable import Experimentation

final class MockProviderTests: XCTestCase {
    func testGetAssignment() async throws {
        let provider = MockProvider(assignments: [
            "exp_home": Assignment(experimentId: "exp_home", variantName: "treatment"),
        ])
        try await provider.initialize()

        let attrs = UserAttributes(userId: "user-1")
        let assignment = try await provider.getAssignment(experimentId: "exp_home", attributes: attrs)

        XCTAssertNotNil(assignment)
        XCTAssertEqual(assignment?.variantName, "treatment")
    }

    func testGetAssignmentMissing() async throws {
        let provider = MockProvider()
        try await provider.initialize()

        let attrs = UserAttributes(userId: "user-1")
        let assignment = try await provider.getAssignment(experimentId: "nonexistent", attributes: attrs)
        XCTAssertNil(assignment)
    }

    func testSetAssignment() async throws {
        let provider = MockProvider()
        try await provider.initialize()

        await provider.setAssignment(experimentId: "exp_recs", variantName: "v2")
        let attrs = UserAttributes(userId: "user-1")
        let assignment = try await provider.getAssignment(experimentId: "exp_recs", attributes: attrs)

        XCTAssertEqual(assignment?.variantName, "v2")
    }

    func testClientWithMock() async throws {
        let provider = MockProvider(assignments: [
            "exp_recs": Assignment(experimentId: "exp_recs", variantName: "treatment"),
        ])
        let client = ExperimentClient(provider: provider)
        try await client.initialize()

        let variant = try await client.getVariant("exp_recs", userId: "user-42")
        XCTAssertEqual(variant, "treatment")

        let missing = try await client.getVariant("missing", userId: "user-42")
        XCTAssertNil(missing)

        await client.close()
    }
}
