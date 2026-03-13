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

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.io.OutputStreamWriter
import java.net.HttpURLConnection
import java.net.URL

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
 * Calls the Assignment Service via JSON HTTP.
 */
class RemoteProvider(
    private val baseUrl: String,
    private val timeoutMs: Long = 2000L,
) : AssignmentProvider {

    private var initialized = false

    override suspend fun initialize() {
        initialized = true
    }

    override suspend fun getAssignment(experimentId: String, attrs: UserAttributes): Assignment? {
        check(initialized) { "provider not initialized" }

        val url = "$baseUrl/experimentation.assignment.v1.AssignmentService/GetAssignment"
        val body = JSONObject().apply {
            put("userId", attrs.userId)
            put("experimentId", experimentId)
            put("attributes", JSONObject(attrs.properties.mapValues { (_, v) -> v.toString() }))
        }

        val responseJson = post(url, body) ?: return null
        if (!responseJson.optBoolean("isActive", false)) return null
        val variantId = responseJson.optString("variantId", "")
        if (variantId.isEmpty()) return null

        return Assignment(
            experimentId = responseJson.optString("experimentId", experimentId),
            variantName = variantId,
            payload = parsePayload(responseJson.optString("payloadJson", "")),
            fromCache = false,
        )
    }

    override suspend fun getAllAssignments(attrs: UserAttributes): Map<String, Assignment> {
        check(initialized) { "provider not initialized" }

        val url = "$baseUrl/experimentation.assignment.v1.AssignmentService/GetAssignments"
        val body = JSONObject().apply {
            put("userId", attrs.userId)
            put("attributes", JSONObject(attrs.properties.mapValues { (_, v) -> v.toString() }))
        }

        val responseJson = post(url, body) ?: return emptyMap()
        val assignmentsArray = responseJson.optJSONArray("assignments") ?: return emptyMap()

        val results = mutableMapOf<String, Assignment>()
        for (i in 0 until assignmentsArray.length()) {
            val a = assignmentsArray.getJSONObject(i)
            if (!a.optBoolean("isActive", false)) continue
            val variantId = a.optString("variantId", "")
            if (variantId.isEmpty()) continue
            val expId = a.getString("experimentId")
            results[expId] = Assignment(
                experimentId = expId,
                variantName = variantId,
                payload = parsePayload(a.optString("payloadJson", "")),
                fromCache = false,
            )
        }
        return results
    }

    override suspend fun close() {
        initialized = false
    }

    // -- Private --

    private suspend fun post(urlString: String, body: JSONObject): JSONObject? =
        withContext(Dispatchers.IO) {
            val conn = URL(urlString).openConnection() as HttpURLConnection
            try {
                conn.requestMethod = "POST"
                conn.connectTimeout = timeoutMs.toInt()
                conn.readTimeout = timeoutMs.toInt()
                conn.setRequestProperty("Content-Type", "application/json")
                conn.doOutput = true

                OutputStreamWriter(conn.outputStream, Charsets.UTF_8).use { it.write(body.toString()) }

                if (conn.responseCode != 200) return@withContext null

                val responseText = conn.inputStream.bufferedReader(Charsets.UTF_8).use { it.readText() }
                JSONObject(responseText)
            } finally {
                conn.disconnect()
            }
        }

    private fun parsePayload(jsonString: String): Map<String, Any> {
        if (jsonString.isEmpty()) return emptyMap()
        return try {
            val obj = JSONObject(jsonString)
            val result = mutableMapOf<String, Any>()
            for (key in obj.keys()) {
                result[key] = obj.get(key)
            }
            result
        } catch (_: Exception) {
            emptyMap()
        }
    }
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
