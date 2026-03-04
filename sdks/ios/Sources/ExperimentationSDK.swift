// Experimentation Platform — iOS SDK
// Provider Abstraction pattern (ADR-007) for Swift.
//
// Usage:
//   let client = ExperimentClient(provider: RemoteProvider(baseURL: url))
//   let variant = try await client.getVariant("homepage_recs_v2", userId: "user-123")

import Foundation

// MARK: - Core Types

public struct Assignment: Sendable {
    public let experimentId: String
    public let variantName: String
    public let payload: [String: Any]
    public let fromCache: Bool

    public init(experimentId: String, variantName: String, payload: [String: Any] = [:], fromCache: Bool = false) {
        self.experimentId = experimentId
        self.variantName = variantName
        self.payload = payload
        self.fromCache = fromCache
    }
}

public struct UserAttributes: Sendable {
    public let userId: String
    public let properties: [String: String]

    public init(userId: String, properties: [String: String] = [:]) {
        self.userId = userId
        self.properties = properties
    }
}

// MARK: - Provider Protocol

/// All assignment backends implement this protocol (ADR-007).
public protocol AssignmentProvider: Sendable {
    func initialize() async throws
    func getAssignment(experimentId: String, attributes: UserAttributes) async throws -> Assignment?
    func getAllAssignments(attributes: UserAttributes) async throws -> [String: Assignment]
    func close() async
}

// MARK: - RemoteProvider

/// Calls the Assignment Service via ConnectRPC.
public final class RemoteProvider: AssignmentProvider {
    private let baseURL: URL

    public init(baseURL: URL) {
        self.baseURL = baseURL
    }

    public func initialize() async throws {
        // TODO (Agent-1): Create Connect-Swift transport
    }

    public func getAssignment(experimentId: String, attributes: UserAttributes) async throws -> Assignment? {
        // TODO (Agent-1): Call AssignmentService.GetAssignment
        return nil
    }

    public func getAllAssignments(attributes: UserAttributes) async throws -> [String: Assignment] {
        // TODO (Agent-1): Call AssignmentService.GetAllAssignments
        return [:]
    }

    public func close() async {}
}

// MARK: - MockProvider

/// Returns deterministic assignments for testing.
public final class MockProvider: AssignmentProvider {
    private var assignments: [String: Assignment] = [:]

    public init(assignments: [String: String] = [:]) {
        for (expId, variant) in assignments {
            self.assignments[expId] = Assignment(experimentId: expId, variantName: variant)
        }
    }

    public func initialize() async throws {}

    public func getAssignment(experimentId: String, attributes: UserAttributes) async throws -> Assignment? {
        return assignments[experimentId]
    }

    public func getAllAssignments(attributes: UserAttributes) async throws -> [String: Assignment] {
        return assignments
    }

    public func setAssignment(experimentId: String, variantName: String) {
        assignments[experimentId] = Assignment(experimentId: experimentId, variantName: variantName)
    }

    public func close() async {
        assignments.removeAll()
    }
}

// MARK: - Client

public final class ExperimentClient {
    private let provider: AssignmentProvider
    private let fallback: AssignmentProvider?

    public init(provider: AssignmentProvider, fallback: AssignmentProvider? = nil) {
        self.provider = provider
        self.fallback = fallback
    }

    public func initialize() async throws {
        try await provider.initialize()
        try await fallback?.initialize()
    }

    public func getVariant(_ experimentId: String, userId: String, properties: [String: String] = [:]) async throws -> String? {
        let attrs = UserAttributes(userId: userId, properties: properties)
        do {
            return try await provider.getAssignment(experimentId: experimentId, attributes: attrs)?.variantName
        } catch {
            return try await fallback?.getAssignment(experimentId: experimentId, attributes: attrs)?.variantName
        }
    }

    public func close() async {
        await provider.close()
        await fallback?.close()
    }
}
