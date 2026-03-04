//
// ExperimentClient.swift
// Experimentation Platform — iOS SDK
//
// Implements the Provider Abstraction pattern (ADR-007) with three backends:
//   - RemoteProvider: Calls the Assignment Service via ConnectRPC
//   - LocalProvider:  Evaluates assignments locally using cached config
//   - MockProvider:   Returns deterministic assignments for testing
//
// Usage:
//   let client = ExperimentClient(
//       provider: RemoteProvider(baseURL: URL(string: "https://assignment.example.com")!)
//   )
//   let variant = try await client.getVariant("homepage_recs_v2", userId: "user-123")
//

import Foundation

// MARK: - Types

/// A variant assignment for a single experiment.
public struct Assignment: Sendable, Equatable {
    public let experimentId: String
    public let variantName: String
    public let payload: [String: String]
    public let fromCache: Bool

    public init(experimentId: String, variantName: String, payload: [String: String] = [:], fromCache: Bool = false) {
        self.experimentId = experimentId
        self.variantName = variantName
        self.payload = payload
        self.fromCache = fromCache
    }
}

/// User attributes for targeting evaluation.
public struct UserAttributes: Sendable {
    public let userId: String
    public let properties: [String: String]

    public init(userId: String, properties: [String: String] = [:]) {
        self.userId = userId
        self.properties = properties
    }
}

// MARK: - Provider Protocol

/// Provider abstraction — all assignment backends implement this protocol.
/// See ADR-007 for the design rationale.
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
    private let timeoutSeconds: TimeInterval

    public init(baseURL: URL, timeoutSeconds: TimeInterval = 2.0) {
        self.baseURL = baseURL
        self.timeoutSeconds = timeoutSeconds
    }

    public func initialize() async throws {
        // TODO (Agent-1): Create ConnectRPC client for AssignmentService
    }

    public func getAssignment(experimentId: String, attributes: UserAttributes) async throws -> Assignment? {
        // TODO (Agent-1): Call AssignmentService.GetAssignment via ConnectRPC
        _ = experimentId; _ = attributes
        return nil
    }

    public func getAllAssignments(attributes: UserAttributes) async throws -> [String: Assignment] {
        // TODO (Agent-1): Call AssignmentService.GetAllAssignments
        _ = attributes
        return [:]
    }

    public func close() async {
        // TODO (Agent-1): Close transport
    }
}

// MARK: - MockProvider

/// Returns deterministic assignments for testing.
public actor MockProvider: AssignmentProvider {
    private var assignments: [String: Assignment]

    public init(assignments: [String: Assignment] = [:]) {
        self.assignments = assignments
    }

    public func initialize() async throws {}

    public func getAssignment(experimentId: String, attributes: UserAttributes) async throws -> Assignment? {
        _ = attributes
        return assignments[experimentId]
    }

    public func getAllAssignments(attributes: UserAttributes) async throws -> [String: Assignment] {
        _ = attributes
        return assignments
    }

    /// Override an assignment at runtime (useful in tests).
    public func setAssignment(experimentId: String, variantName: String) {
        assignments[experimentId] = Assignment(experimentId: experimentId, variantName: variantName)
    }

    public func close() async {
        assignments.removeAll()
    }
}

// MARK: - ExperimentClient

/// Main entry point for the iOS SDK.
public final class ExperimentClient: Sendable {
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

    /// Returns the variant name, or nil if not assigned.
    public func getVariant(_ experimentId: String, userId: String, properties: [String: String] = [:]) async throws -> String? {
        let assignment = try await getAssignment(experimentId, userId: userId, properties: properties)
        return assignment?.variantName
    }

    /// Returns the full Assignment with fallback on error.
    public func getAssignment(_ experimentId: String, userId: String, properties: [String: String] = [:]) async throws -> Assignment? {
        let attrs = UserAttributes(userId: userId, properties: properties)
        do {
            return try await provider.getAssignment(experimentId: experimentId, attributes: attrs)
        } catch {
            if let fallback {
                return try await fallback.getAssignment(experimentId: experimentId, attributes: attrs)
            }
            throw error
        }
    }

    public func close() async {
        await provider.close()
        await fallback?.close()
    }
}
