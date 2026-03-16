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

/// Calls the Assignment Service via JSON HTTP.
public final class RemoteProvider: AssignmentProvider {
    private let baseURL: URL
    private let timeoutSeconds: TimeInterval
    private var session: URLSession?

    public init(baseURL: URL, timeoutSeconds: TimeInterval = 2.0) {
        self.baseURL = baseURL
        self.timeoutSeconds = timeoutSeconds
    }

    public func initialize() async throws {
        let config = URLSessionConfiguration.default
        config.timeoutIntervalForRequest = timeoutSeconds
        session = URLSession(configuration: config)
    }

    public func getAssignment(experimentId: String, attributes: UserAttributes) async throws -> Assignment? {
        let url = baseURL.appendingPathComponent(
            "experimentation.assignment.v1.AssignmentService/GetAssignment"
        )
        let body: [String: Any] = [
            "userId": attributes.userId,
            "experimentId": experimentId,
            "attributes": attributes.properties,
        ]
        let data = try await post(url: url, body: body)
        guard let json = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            return nil
        }
        guard json["isActive"] as? Bool == true,
              let variantId = json["variantId"] as? String,
              !variantId.isEmpty else {
            return nil
        }
        let experimentIdResp = json["experimentId"] as? String ?? experimentId
        let payloadJson = json["payloadJson"] as? String ?? ""
        let payload = Self.parsePayload(payloadJson)
        return Assignment(
            experimentId: experimentIdResp,
            variantName: variantId,
            payload: payload,
            fromCache: false
        )
    }

    public func getAllAssignments(attributes: UserAttributes) async throws -> [String: Assignment] {
        let url = baseURL.appendingPathComponent(
            "experimentation.assignment.v1.AssignmentService/GetAssignments"
        )
        let body: [String: Any] = [
            "userId": attributes.userId,
            "attributes": attributes.properties,
        ]
        let data = try await post(url: url, body: body)
        guard let json = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let assignments = json["assignments"] as? [[String: Any]] else {
            return [:]
        }
        var results: [String: Assignment] = [:]
        for a in assignments {
            guard a["isActive"] as? Bool == true,
                  let variantId = a["variantId"] as? String,
                  !variantId.isEmpty,
                  let expId = a["experimentId"] as? String else {
                continue
            }
            let payloadJson = a["payloadJson"] as? String ?? ""
            let payload = Self.parsePayload(payloadJson)
            results[expId] = Assignment(
                experimentId: expId,
                variantName: variantId,
                payload: payload,
                fromCache: false
            )
        }
        return results
    }

    public func close() async {
        session?.invalidateAndCancel()
        session = nil
    }

    // MARK: - Private

    private func post(url: URL, body: [String: Any]) async throws -> Data {
        guard let session else {
            throw NSError(domain: "ExperimentationSDK", code: -1, userInfo: [
                NSLocalizedDescriptionKey: "provider not initialized"
            ])
        }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONSerialization.data(withJSONObject: body)
        let (data, response) = try await session.data(for: request)
        guard let httpResp = response as? HTTPURLResponse, httpResp.statusCode == 200 else {
            return Data()
        }
        return data
    }

    private static func parsePayload(_ jsonString: String) -> [String: String] {
        guard !jsonString.isEmpty,
              let data = jsonString.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            return [:]
        }
        var result: [String: String] = [:]
        for (key, value) in obj {
            result[key] = "\(value)"
        }
        return result
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
