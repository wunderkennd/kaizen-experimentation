"""
Integration Guide and User Experience Guide content for DocMost.

This module contains the markdown content for the Integration Guide and
User Experience Guide spaces. It is imported by populate_docmost.py.
"""

# =============================================================================
# INTEGRATION GUIDE CONTENT
# =============================================================================

INTEGRATION_OVERVIEW = """\
# Integration Guide — Kaizen Experimentation Platform

Welcome to the Kaizen Experimentation Platform integration guide. This \
documentation covers everything you need to integrate Kaizen into your \
application, from initial setup to production deployment.

## Who This Guide Is For

| Audience | Start Here |
|----------|-----------|
| **Backend Engineers** | SDK Integration (Go, Python) > Event Pipeline > API Reference |
| **Frontend Engineers** | SDK Integration (Web, iOS, Android) > Feature Flags > UI Components |
| **Data Scientists** | Experiment Lifecycle > Metrics & Analysis > Statistical Methods |
| **Platform/DevOps** | Infrastructure Setup > Deployment > Monitoring |
| **Product Managers** | Quick Start > Creating Experiments > Reading Results |

## Integration Architecture

```
+-------------------------------------------------------------+
|                    Your Application                          |
|  +----------+  +----------+  +----------+  +----------+    |
|  | Web SDK  |  | iOS SDK  |  |Android SDK|  |Server SDK|    |
|  +----+-----+  +----+-----+  +----+-----+  +----+-----+    |
|       |              |              |              |          |
+-------+--------------+--------------+--------------+----------+
        |              |              |              |
        v              v              v              v
+-------------------------------------------------------------+
|              Kaizen Experimentation Platform                  |
|                                                               |
|  +-------------+  +--------------+  +------------------+     |
|  | M1 Assignment|  | M7 Feature   |  | M5 Management    |     |
|  | Service      |  | Flag Service |  | Service (API)    |     |
|  | (Rust)       |  | (Go)         |  | (Go)             |     |
|  +------+-------+  +------+-------+  +--------+---------+     |
|         |                 |                    |              |
|  +------+--------+--------+--------+-----------+----------+  |
|  |              Internal Services                          |  |
|  |  M2 Pipeline | M3 Metrics | M4a Analysis | M4b Bandit  |  |
|  +---------------------------------------------------------+  |
|                                                               |
|  +---------------------------------------------------------+  |
|  |  Infrastructure: PostgreSQL | Kafka | Delta Lake | Redis |  |
|  +---------------------------------------------------------+  |
+---------------------------------------------------------------+
```

## Integration Steps (High Level)

1. **Choose your SDK** — Web, iOS, Android, Server-Go, or Server-Python
2. **Configure the provider** — Point to the Assignment Service endpoint
3. **Get variant assignments** — Call `getVariant()` in your application code
4. **Log exposure events** — Send events to the Event Pipeline (M2)
5. **Create experiments** — Use the Management API or Dashboard UI
6. **Monitor results** — View analysis in the Dashboard

## Key Concepts

- **Provider Abstraction (ADR-007)**: All SDKs use a pluggable provider \
pattern with Remote, Local, and Mock backends
- **Deterministic Bucketing**: MurmurHash3 ensures the same user always \
gets the same variant
- **Fallback Chain**: Configure a fallback provider for resilience \
(e.g., Remote then Local)
- **ConnectRPC**: All service-to-service communication uses ConnectRPC \
(ADR-010)
- **Schema-First**: All APIs are defined in Protobuf with buf toolchain \
enforcement
"""

SDK_OVERVIEW = """\
# SDK Overview

Kaizen provides official SDKs for five platforms. All SDKs implement the \
**Provider Abstraction** pattern (ADR-007) with three interchangeable backends:

| SDK | Language | Package | Use Case |
|-----|----------|---------|----------|
| **Web SDK** | TypeScript | `@experimentation/sdk-web` | Browser apps, SPAs, Next.js |
| **iOS SDK** | Swift | `ExperimentationSDK` | Native iOS apps (Swift 5.9+) |
| **Android SDK** | Kotlin | `com.experimentation:sdk` | Native Android apps |
| **Server Go** | Go | `experimentation` | Go microservices, API servers |
| **Server Python** | Python | `experimentation` | Python services, ML pipelines |

## Provider Types

### RemoteProvider
Calls the Assignment Service (M1) via ConnectRPC/JSON HTTP. This is the \
**recommended** provider for production.

- **Pros**: Always up-to-date, supports bandit experiments, no local state
- **Cons**: Requires network call, adds latency (~2-10ms)
- **Endpoint**: \
`POST /experimentation.assignment.v1.AssignmentService/GetAssignment`

### LocalProvider
Evaluates assignments locally using cached experiment configs and MurmurHash3. \
Useful as a fallback or for offline scenarios.

- **Pros**: Zero latency, works offline, no network dependency
- **Cons**: Requires config sync, cannot support bandit experiments
- **Hash**: MurmurHash3 x86_32 with `{userId}\\x00{salt}` key format

### MockProvider
Returns deterministic, predefined assignments. Essential for unit and \
integration testing.

- **Pros**: Fully deterministic, no external dependencies
- **Cons**: Not for production use
- **Use**: Set up specific variant assignments for test scenarios

## Fallback Chain

All SDKs support a fallback provider chain (ADR-007). If the primary \
provider fails, the SDK automatically falls back:

```
RemoteProvider (primary)
    |
    +-- Success -> return assignment
    |
    +-- Error -> LocalProvider (fallback)
                    |
                    +-- Success -> return cached assignment
                    |
                    +-- Error -> return null (no assignment)
```

## Common API Surface

Every SDK exposes the same core methods:

| Method | Description |
|--------|-------------|
| `initialize()` | Prepare the provider (connect, fetch config) |
| `getVariant(experimentId)` | Get variant name for a single experiment |
| `getAssignment(experimentId)` | Get full assignment with payload |
| `getAllAssignments()` | Bulk-fetch all active experiment assignments |
| `close()` / `destroy()` | Release resources |
"""

WEB_SDK = """\
# Web SDK (TypeScript)

The Web SDK is designed for browser-based applications including SPAs, \
Next.js apps, and any JavaScript/TypeScript frontend.

## Installation

```bash
npm install @experimentation/sdk-web
# or
yarn add @experimentation/sdk-web
```

## Quick Start

```typescript
import { ExperimentClient, RemoteProvider } from '@experimentation/sdk-web';

// 1. Create the client
const client = new ExperimentClient({
  provider: new RemoteProvider({
    baseUrl: 'https://assignment.example.com',
  }),
  userId: 'user-123',
});

// 2. Get a variant
const variant = await client.getVariant('homepage_recs_v2');

// 3. Use the variant
if (variant === 'treatment') {
  showNewRecommendations();
} else {
  showDefaultRecommendations();
}

// 4. Clean up when done
await client.destroy();
```

## Configuration

### ExperimentClientConfig

| Property | Type | Required | Description |
|----------|------|----------|-------------|
| `provider` | `AssignmentProvider` | Yes | Primary assignment provider |
| `userId` | `string` | Yes | Unique user identifier |
| `attributes` | `Record<string, string>` | No | User attributes for targeting |
| `fallbackProvider` | `AssignmentProvider` | No | Fallback if primary fails |

### RemoteProviderConfig

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `baseUrl` | `string` | — | Assignment Service URL |
| `timeoutMs` | `number` | `2000` | Request timeout in ms |
| `retries` | `number` | `1` | Retry count on transient failures |

## Using with Fallback

```typescript
import {
  ExperimentClient,
  RemoteProvider,
  LocalProvider,
} from '@experimentation/sdk-web';

const client = new ExperimentClient({
  provider: new RemoteProvider({
    baseUrl: 'https://assignment.example.com',
    timeoutMs: 2000,
  }),
  fallbackProvider: new LocalProvider({
    experiments: cachedExperimentConfigs,
  }),
  userId: currentUser.id,
  attributes: {
    country: currentUser.country,
    plan: currentUser.subscriptionPlan,
  },
});
```

## React Integration Example

```tsx
import { createContext, useContext, useEffect, useState } from 'react';
import { ExperimentClient, RemoteProvider } from '@experimentation/sdk-web';

const ExperimentContext = createContext<ExperimentClient | null>(null);

export function ExperimentProvider({ userId, children }) {
  const [client, setClient] = useState<ExperimentClient | null>(null);

  useEffect(() => {
    const c = new ExperimentClient({
      provider: new RemoteProvider({
        baseUrl: process.env.NEXT_PUBLIC_ASSIGNMENT_URL!,
      }),
      userId,
    });
    c.initialize().then(() => setClient(c));
    return () => { c.destroy(); };
  }, [userId]);

  return (
    <ExperimentContext.Provider value={client}>
      {children}
    </ExperimentContext.Provider>
  );
}

export function useExperiment(experimentId: string) {
  const client = useContext(ExperimentContext);
  const [variant, setVariant] = useState<string | null>(null);

  useEffect(() => {
    if (!client) return;
    client.getVariant(experimentId).then(setVariant);
  }, [client, experimentId]);

  return variant;
}

// Usage in a component:
function HomepageRecs() {
  const variant = useExperiment('homepage_recs_v2');

  if (variant === 'treatment') {
    return <NewRecsCarousel />;
  }
  return <DefaultRecsCarousel />;
}
```

## Testing with MockProvider

```typescript
import { ExperimentClient, MockProvider } from '@experimentation/sdk-web';

describe('Homepage', () => {
  it('shows new recs for treatment variant', async () => {
    const client = new ExperimentClient({
      provider: new MockProvider([
        { experimentId: 'homepage_recs_v2', variantName: 'treatment' },
      ]),
      userId: 'test-user',
    });

    const variant = await client.getVariant('homepage_recs_v2');
    expect(variant).toBe('treatment');
  });
});
```
"""

SERVER_GO_SDK = """\
# Server Go SDK

The Go SDK is designed for backend services, API servers, and microservices \
written in Go.

## Installation

```bash
go get github.com/org/experimentation/sdks/server-go
```

## Quick Start

```go
package main

import (
    "context"
    "fmt"
    "log"

    experimentation "github.com/org/experimentation/sdks/server-go"
)

func main() {
    ctx := context.Background()

    // 1. Create client with remote provider
    client, err := experimentation.NewClient(ctx, experimentation.Config{
        Provider: experimentation.NewRemoteProvider(
            "https://assignment.example.com",
        ),
    })
    if err != nil {
        log.Fatal(err)
    }
    defer client.Close()

    // 2. Get variant for a user
    variant, err := client.GetVariant(
        ctx, "homepage_recs_v2", "user-123", nil,
    )
    if err != nil {
        log.Fatal(err)
    }

    // 3. Use the variant
    fmt.Printf("User assigned to variant: %s\\n", variant)
}
```

## Provider Types

### RemoteProvider

```go
provider := experimentation.NewRemoteProvider(
    "https://assignment.example.com",
)
```

Calls the Assignment Service via JSON HTTP POST. Default timeout: 2000ms.

### LocalProvider

```go
provider := experimentation.NewLocalProvider(
    []experimentation.ExperimentConfig{
        {
            ExperimentID:    "homepage_recs_v2",
            HashSalt:        "salt-abc123",
            LayerName:       "homepage",
            TotalBuckets:    10000,
            AllocationStart: 0,
            AllocationEnd:   9999,
            Variants: []experimentation.VariantConfig{
                {Name: "control", TrafficFraction: 0.5, IsControl: true},
                {Name: "treatment", TrafficFraction: 0.5},
            },
        },
    },
)
```

Evaluates assignments locally using MurmurHash3 via CGo bridge to the \
Rust `experimentation-hash` crate.

### MockProvider

```go
provider := experimentation.NewMockProvider(
    map[string]*experimentation.Assignment{
        "homepage_recs_v2": {
            ExperimentID: "homepage_recs_v2",
            VariantName:  "treatment",
        },
    },
)
```

### Fallback Chain

```go
client, err := experimentation.NewClient(ctx, experimentation.Config{
    Provider:         experimentation.NewRemoteProvider(
        "https://assignment.example.com",
    ),
    FallbackProvider: experimentation.NewLocalProvider(cachedConfigs),
})
```

## HTTP Middleware Example

```go
func ExperimentMiddleware(
    client *experimentation.Client,
) func(http.Handler) http.Handler {
    return func(next http.Handler) http.Handler {
        return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
            userID := r.Header.Get("X-User-ID")
            if userID == "" {
                next.ServeHTTP(w, r)
                return
            }

            assignment, err := client.GetAssignment(
                r.Context(),
                "homepage_recs_v2",
                userID,
                map[string]any{"country": r.Header.Get("X-Country")},
            )
            if err == nil && assignment != nil {
                ctx := context.WithValue(
                    r.Context(),
                    "experiment_variant",
                    assignment.VariantName,
                )
                r = r.WithContext(ctx)
            }

            next.ServeHTTP(w, r)
        })
    }
}
```

## Testing

```go
func TestRecommendationHandler(t *testing.T) {
    mock := experimentation.NewMockProvider(nil)
    mock.SetAssignment("homepage_recs_v2", "treatment")

    client, _ := experimentation.NewClient(
        context.Background(),
        experimentation.Config{Provider: mock},
    )

    variant, _ := client.GetVariant(
        context.Background(), "homepage_recs_v2", "user-123", nil,
    )
    if variant != "treatment" {
        t.Errorf("expected treatment, got %s", variant)
    }
}
```
"""

SERVER_PYTHON_SDK = """\
# Server Python SDK

The Python SDK is designed for Python backend services, ML pipelines, and \
data processing applications.

## Installation

```bash
pip install experimentation
# or with poetry
poetry add experimentation
```

**Dependencies**: `httpx` (for RemoteProvider), `mmh3` (for LocalProvider)

## Quick Start

```python
import asyncio
from experimentation.client import ExperimentClient
from experimentation.providers import RemoteProvider

async def main():
    # 1. Create client
    client = ExperimentClient(
        provider=RemoteProvider(
            base_url="https://assignment.example.com",
        ),
    )
    await client.initialize()

    # 2. Get variant
    variant = await client.get_variant("homepage_recs_v2", "user-123")

    # 3. Use the variant
    if variant == "treatment":
        result = new_recommendations()
    else:
        result = default_recommendations()

    # 4. Clean up
    await client.close()
    return result

asyncio.run(main())
```

## Provider Types

### RemoteProvider

```python
from experimentation.providers import RemoteProvider

provider = RemoteProvider(
    base_url="https://assignment.example.com",
    timeout_ms=2000,
)
```

Uses `httpx.AsyncClient` for non-blocking HTTP calls to the Assignment Service.

### LocalProvider

```python
from experimentation.providers import LocalProvider
from experimentation.types import ExperimentConfig, VariantConfig

provider = LocalProvider(configs=[
    ExperimentConfig(
        experiment_id="homepage_recs_v2",
        hash_salt="salt-abc123",
        layer_name="homepage",
        total_buckets=10_000,
        allocation_start=0,
        allocation_end=9999,
        variants=[
            VariantConfig(
                name="control",
                traffic_fraction=0.5,
                is_control=True,
            ),
            VariantConfig(
                name="treatment",
                traffic_fraction=0.5,
            ),
        ],
    ),
])
```

Uses `mmh3` (MurmurHash3) for deterministic bucketing identical to the \
Rust backend.

### MockProvider

```python
from experimentation.providers import MockProvider
from experimentation.types import Assignment

provider = MockProvider(assignments={
    "homepage_recs_v2": Assignment(
        experiment_id="homepage_recs_v2",
        variant_name="treatment",
    ),
})
```

### Fallback Chain

```python
client = ExperimentClient(
    provider=RemoteProvider(
        base_url="https://assignment.example.com",
    ),
    fallback_provider=LocalProvider(configs=cached_configs),
)
```

## FastAPI Integration Example

```python
from contextlib import asynccontextmanager
from fastapi import FastAPI, Request
from experimentation.client import ExperimentClient
from experimentation.providers import RemoteProvider

experiment_client: ExperimentClient | None = None

@asynccontextmanager
async def lifespan(app: FastAPI):
    global experiment_client
    experiment_client = ExperimentClient(
        provider=RemoteProvider(
            base_url="https://assignment.example.com",
        ),
    )
    await experiment_client.initialize()
    yield
    await experiment_client.close()

app = FastAPI(lifespan=lifespan)

async def get_variant(request: Request, experiment_id: str):
    user_id = request.headers.get("X-User-ID")
    if not user_id or not experiment_client:
        return None
    return await experiment_client.get_variant(experiment_id, user_id)

@app.get("/recommendations")
async def recommendations(request: Request):
    variant = await get_variant(request, "homepage_recs_v2")
    if variant == "treatment":
        return {"recs": get_new_recommendations()}
    return {"recs": get_default_recommendations()}
```

## Testing with pytest

```python
import pytest
from experimentation.client import ExperimentClient
from experimentation.providers import MockProvider
from experimentation.types import Assignment

@pytest.fixture
async def experiment_client():
    mock = MockProvider(assignments={
        "homepage_recs_v2": Assignment(
            experiment_id="homepage_recs_v2",
            variant_name="treatment",
        ),
    })
    client = ExperimentClient(provider=mock)
    await client.initialize()
    yield client
    await client.close()

@pytest.mark.asyncio
async def test_treatment_variant(experiment_client):
    variant = await experiment_client.get_variant(
        "homepage_recs_v2", "user-123",
    )
    assert variant == "treatment"
```
"""

MOBILE_SDKS = """\
# Mobile SDKs (iOS & Android)

## iOS SDK (Swift)

### Installation

Add to your `Package.swift`:

```swift
dependencies: [
    .package(
        url: "https://github.com/org/experimentation-ios.git",
        from: "1.0.0"
    ),
]
```

### Quick Start

```swift
import ExperimentationSDK

// 1. Create client
let client = ExperimentClient(
    provider: RemoteProvider(
        baseURL: URL(string: "https://assignment.example.com")!,
        timeoutSeconds: 2.0
    )
)
try await client.initialize()

// 2. Get variant
let variant = try await client.getVariant(
    "homepage_recs_v2",
    userId: "user-123",
    properties: ["country": "US", "plan": "premium"]
)

// 3. Use the variant
if variant == "treatment" {
    showNewRecommendations()
} else {
    showDefaultRecommendations()
}

// 4. Clean up
await client.close()
```

### SwiftUI Integration

```swift
import SwiftUI
import ExperimentationSDK

@MainActor
class ExperimentViewModel: ObservableObject {
    private let client: ExperimentClient
    @Published var variant: String?

    init() {
        self.client = ExperimentClient(
            provider: RemoteProvider(
                baseURL: URL(
                    string: "https://assignment.example.com"
                )!
            )
        )
    }

    func loadVariant(
        experimentId: String,
        userId: String
    ) async {
        do {
            try await client.initialize()
            variant = try await client.getVariant(
                experimentId, userId: userId
            )
        } catch {
            variant = nil // Default to control
        }
    }
}

struct RecommendationsView: View {
    @StateObject private var vm = ExperimentViewModel()
    let userId: String

    var body: some View {
        Group {
            if vm.variant == "treatment" {
                NewRecsView()
            } else {
                DefaultRecsView()
            }
        }
        .task {
            await vm.loadVariant(
                experimentId: "homepage_recs_v2",
                userId: userId
            )
        }
    }
}
```

### Testing (iOS)

```swift
let mock = MockProvider(assignments: [
    "homepage_recs_v2": "treatment"
])
let client = ExperimentClient(provider: mock)
try await client.initialize()

let variant = try await client.getVariant(
    "homepage_recs_v2", userId: "test-user"
)
XCTAssertEqual(variant, "treatment")
```

---

## Android SDK (Kotlin)

### Installation

Add to your `build.gradle.kts`:

```kotlin
dependencies {
    implementation("com.experimentation:sdk:1.0.0")
}
```

### Quick Start

```kotlin
import com.experimentation.sdk.*

// 1. Create client
val client = ExperimentClient(
    provider = RemoteProvider(
        baseUrl = "https://assignment.example.com",
        timeoutMs = 2000
    )
)
client.initialize()

// 2. Get variant
val variant = client.getVariant(
    experimentId = "homepage_recs_v2",
    userId = "user-123",
    attributes = mapOf("country" to "US")
)

// 3. Use the variant
when (variant) {
    "treatment" -> showNewRecommendations()
    else -> showDefaultRecommendations()
}

// 4. Clean up
client.close()
```

### Jetpack Compose Integration

```kotlin
@Composable
fun RecommendationsScreen(userId: String) {
    val variant by produceState<String?>(initialValue = null) {
        val client = ExperimentClient(
            provider = RemoteProvider(
                baseUrl = BuildConfig.ASSIGNMENT_URL
            )
        )
        client.initialize()
        value = client.getVariant("homepage_recs_v2", userId)
    }

    when (variant) {
        "treatment" -> NewRecsCarousel()
        else -> DefaultRecsCarousel()
    }
}
```
"""

API_REFERENCE = """\
# API Reference

All Kaizen APIs use **ConnectRPC** (ADR-010) with JSON encoding over HTTP. \
Every endpoint accepts `POST` requests with `Content-Type: application/json`.

## Assignment Service (M1)

Base URL: `https://assignment.example.com`

### GetAssignment

Get a variant assignment for a single experiment.

**Endpoint**: \
`POST /experimentation.assignment.v1.AssignmentService/GetAssignment`

**Request**:
```json
{
  "userId": "user-123",
  "experimentId": "homepage_recs_v2",
  "sessionId": "session-abc",
  "attributes": {
    "country": "US",
    "plan": "premium"
  }
}
```

**Response**:
```json
{
  "experimentId": "homepage_recs_v2",
  "variantId": "treatment",
  "payloadJson": "{\\"algorithm\\": \\"collaborative_filtering_v2\\"}",
  "assignmentProbability": 0.5,
  "isActive": true
}
```

| Field | Type | Description |
|-------|------|-------------|
| `experimentId` | string | Experiment identifier |
| `variantId` | string | Assigned variant name |
| `payloadJson` | string | JSON payload from variant config |
| `assignmentProbability` | double | Assignment probability (for IPW in bandits) |
| `isActive` | bool | Whether the experiment is actively serving |

### GetAssignments (Bulk)

Get all assignments for a user across all active experiments. Used by SDKs \
for bulk-fetch on app startup.

**Endpoint**: \
`POST /experimentation.assignment.v1.AssignmentService/GetAssignments`

**Request**:
```json
{
  "userId": "user-123",
  "sessionId": "session-abc",
  "attributes": {
    "country": "US"
  }
}
```

**Response**:
```json
{
  "assignments": [
    {
      "experimentId": "homepage_recs_v2",
      "variantId": "treatment",
      "payloadJson": "{}",
      "assignmentProbability": 0.5,
      "isActive": true
    },
    {
      "experimentId": "search_ranking_v1",
      "variantId": "control",
      "payloadJson": "{}",
      "assignmentProbability": 0.5,
      "isActive": true
    }
  ]
}
```

### GetInterleavedList

Construct an interleaved list from multiple algorithm outputs (for \
interleaving experiments).

**Endpoint**: \
`POST /experimentation.assignment.v1.AssignmentService/GetInterleavedList`

**Request**:
```json
{
  "experimentId": "search_interleaving_v1",
  "userId": "user-123",
  "algorithmLists": {
    "algo_a": { "itemIds": ["item-1", "item-2", "item-3"] },
    "algo_b": { "itemIds": ["item-2", "item-4", "item-1"] }
  }
}
```

**Response**:
```json
{
  "mergedList": ["item-1", "item-2", "item-4", "item-3"],
  "provenance": {
    "item-1": "algo_a",
    "item-2": "algo_b",
    "item-4": "algo_b",
    "item-3": "algo_a"
  }
}
```

---

## Management Service (M5)

Base URL: `https://management.example.com`

### CreateExperiment

**Endpoint**: \
`POST /experimentation.management.v1.ExperimentManagementService/CreateExperiment`

**Request**:
```json
{
  "experiment": {
    "name": "homepage_recs_v2",
    "description": "Test collaborative filtering v2 vs. current algorithm",
    "ownerEmail": "alice@streamco.com",
    "type": "EXPERIMENT_TYPE_AB",
    "variants": [
      {
        "name": "control",
        "trafficFraction": 0.5,
        "isControl": true,
        "payloadJson": "{}"
      },
      {
        "name": "treatment",
        "trafficFraction": 0.5,
        "isControl": false,
        "payloadJson": "{\\"algorithm\\": \\"cf_v2\\"}"
      }
    ],
    "layerId": "layer-homepage",
    "primaryMetricId": "click_through_rate",
    "secondaryMetricIds": ["watch_time", "completion_rate"],
    "guardrailConfigs": [
      {
        "metricId": "rebuffer_rate",
        "threshold": 0.02,
        "consecutiveBreachesRequired": 2
      }
    ],
    "guardrailAction": "GUARDRAIL_ACTION_AUTO_PAUSE"
  }
}
```

### Lifecycle Transitions

| Endpoint | Transition | Description |
|----------|-----------|-------------|
| `StartExperiment` | DRAFT to RUNNING | Validates config, warms bandit, confirms metrics |
| `PauseExperiment` | RUNNING to PAUSED | Manual or auto-pause on guardrail breach |
| `ResumeExperiment` | PAUSED to RUNNING | Resume after investigation |
| `ConcludeExperiment` | RUNNING to CONCLUDED | Triggers final analysis |
| `ArchiveExperiment` | CONCLUDED to ARCHIVED | Retained for historical reference |

### ListExperiments

**Endpoint**: \
`POST /experimentation.management.v1.ExperimentManagementService/ListExperiments`

**Request**:
```json
{
  "pageSize": 20,
  "pageToken": "",
  "stateFilter": "EXPERIMENT_STATE_RUNNING",
  "typeFilter": "EXPERIMENT_TYPE_AB",
  "ownerEmailFilter": "alice@streamco.com"
}
```

---

## Feature Flag Service (M7)

Base URL: `https://flags.example.com`

### EvaluateFlag

**Endpoint**: \
`POST /experimentation.flags.v1.FeatureFlagService/EvaluateFlag`

**Request**:
```json
{
  "flagId": "dark-mode-enabled",
  "userId": "user-123",
  "attributes": {
    "country": "US",
    "plan": "premium"
  }
}
```

**Response**:
```json
{
  "flagId": "dark-mode-enabled",
  "value": "true",
  "variantId": "enabled"
}
```

### Flag Types

| Type | Description | Example Value |
|------|-------------|---------------|
| `BOOLEAN` | True/false toggle | `"true"` |
| `STRING` | String value | `"variant-a"` |
| `NUMERIC` | Numeric value | `"42"` |
| `JSON` | JSON object | `"{\\"theme\\": \\"dark\\"}"` |

### PromoteToExperiment

Convert a feature flag into a tracked experiment with statistical analysis:

```json
{
  "flagId": "dark-mode-enabled",
  "experimentType": "EXPERIMENT_TYPE_AB",
  "primaryMetricId": "session_duration",
  "secondaryMetricIds": ["page_views", "bounce_rate"]
}
```
"""

EVENT_PIPELINE = """\
# Event Pipeline Integration

The Event Pipeline (M2) handles event ingestion, validation, and publishing \
to Kafka for downstream processing by the Metrics Engine (M3) and Bandit \
Service (M4b).

## Event Flow

```
Your App -> SDK -> Event Pipeline (M2) -> Kafka -> M3 Metrics / M4b Bandit
                                               -> Delta Lake (storage)
```

## Event Types

### Exposure Events

Logged automatically when a user is assigned to a variant. Required for \
accurate analysis.

```json
{
  "eventType": "exposure",
  "experimentId": "homepage_recs_v2",
  "userId": "user-123",
  "variantId": "treatment",
  "timestamp": "2026-03-13T18:00:00Z",
  "sessionId": "session-abc",
  "assignmentProbability": 0.5
}
```

### Metric Events

Custom events that feed into metric computation. These are the raw signals \
that M3 aggregates.

```json
{
  "eventType": "metric",
  "userId": "user-123",
  "metricId": "click_through_rate",
  "value": 1.0,
  "timestamp": "2026-03-13T18:05:00Z",
  "properties": {
    "contentId": "movie-456",
    "position": 3
  }
}
```

### Reward Events (Bandit)

For bandit experiments, reward events feed the policy learning loop:

```json
{
  "eventType": "reward",
  "experimentId": "content_bandit_v1",
  "userId": "user-123",
  "armId": "arm-collaborative",
  "reward": 1.0,
  "timestamp": "2026-03-13T18:10:00Z",
  "context": {
    "genre_preference": "sci-fi",
    "time_of_day": "evening"
  }
}
```

## Kafka Topics

| Topic | Producer | Consumer | Description |
|-------|----------|----------|-------------|
| `exposures` | M2 | M3, Delta Lake | Variant assignment exposures |
| `metric_events` | M2 | M3, Delta Lake | Raw metric signals |
| `reward_events` | M2 | M4b, Delta Lake | Bandit reward signals |
| `guardrail_alerts` | M3 | M5 | Guardrail breach notifications |

## Validation Rules

M2 validates all incoming events before publishing:

- **Schema validation**: Events must match the Protobuf schema
- **Deduplication**: Events with duplicate `(userId, experimentId, timestamp)` \
are dropped
- **Timestamp bounds**: Events older than 7 days or in the future are rejected
- **Required fields**: `userId`, `eventType`, and `timestamp` are always required
- **NaN/overflow check**: Numeric values are validated (fail-fast principle)

## Best Practices

1. **Log exposures at assignment time**, not at render time — this prevents \
selection bias
2. **Include `assignmentProbability`** for bandit experiments — required for \
IPW-adjusted analysis
3. **Use consistent `userId`** across all events — mismatched IDs break analysis
4. **Batch events** when possible to reduce network overhead
5. **Handle failures gracefully** — buffer events locally and retry on \
transient errors
"""

FEATURE_FLAGS = """\
# Feature Flags

The Feature Flag Service (M7) provides a lightweight way to control feature \
rollouts before committing to a full experiment. Flags use the same \
deterministic MurmurHash3 bucketing as the Assignment Service, ensuring \
consistent user experiences.

## Feature Flag vs. Experiment

| Aspect | Feature Flag | Experiment |
|--------|-------------|------------|
| **Purpose** | Gradual rollout, kill switch | Measure causal impact |
| **Analysis** | No statistical analysis | Full statistical analysis |
| **Metrics** | Not tracked | Primary + secondary + guardrails |
| **Lifecycle** | Enable/disable | DRAFT to RUNNING to CONCLUDED |
| **Promotion** | Can promote to experiment | — |

## Creating a Flag

```json
POST /experimentation.flags.v1.FeatureFlagService/CreateFlag

{
  "flag": {
    "name": "dark-mode",
    "description": "Enable dark mode UI",
    "type": "FLAG_TYPE_BOOLEAN",
    "defaultValue": "false",
    "enabled": true,
    "rolloutPercentage": 0.1,
    "variants": [
      { "variantId": "disabled", "value": "false", "trafficFraction": 0.9 },
      { "variantId": "enabled", "value": "true", "trafficFraction": 0.1 }
    ]
  }
}
```

## Gradual Rollout Pattern

```
Day 1:  rolloutPercentage = 0.01  (1% of users)
Day 3:  rolloutPercentage = 0.05  (5% of users)
Day 7:  rolloutPercentage = 0.25  (25% of users)
Day 14: rolloutPercentage = 1.0   (100% - full rollout)
```

Because bucketing is deterministic, users who were in the 1% group on Day 1 \
remain in the group at 5%, 25%, and 100%. No user experiences a "flip" \
during rollout.

## Promoting to Experiment

When you want to measure the causal impact of a feature flag:

```json
POST /experimentation.flags.v1.FeatureFlagService/PromoteToExperiment

{
  "flagId": "dark-mode",
  "experimentType": "EXPERIMENT_TYPE_AB",
  "primaryMetricId": "session_duration",
  "secondaryMetricIds": ["page_views", "bounce_rate"]
}
```

This creates a full experiment in M5 with:
- The flag's current variants as experiment variants
- Statistical analysis via M4a
- Guardrail monitoring
- Full lifecycle management

## Using Flags in Code

```typescript
// Web SDK
const darkMode = await client.evaluateFlag('dark-mode', userId);
if (darkMode.value === 'true') {
  enableDarkMode();
}
```

```go
// Go SDK
resp, _ := flagsClient.EvaluateFlag(ctx, &flagsv1.EvaluateFlagRequest{
    FlagId: "dark-mode",
    UserId: userID,
})
if resp.Value == "true" {
    enableDarkMode()
}
```

```python
# Python
resp = await flags_client.evaluate_flag("dark-mode", user_id)
if resp.value == "true":
    enable_dark_mode()
```
"""

EXPERIMENT_TYPES_DOC = """\
# Experiment Types

Kaizen supports 8 experiment types, each optimized for different SVOD use \
cases. The experiment type determines how M1 assigns variants, how M4a \
analyzes results, and what validation gates M5 enforces.

## A/B Test (AB)

The classic randomized controlled trial. Users are deterministically assigned \
to control or treatment using MurmurHash3 bucketing.

**Use case**: Testing a new recommendation algorithm, UI change, or pricing \
model.

**Requirements**:
- Minimum 2 variants (1 control + 1 treatment)
- Traffic fractions must sum to 1.0
- Primary metric required

**Analysis**: Welch's t-test, optional CUPED variance reduction, optional \
sequential testing (mSPRT or GST)

---

## Multivariate (MULTIVARIATE)

Test multiple factors simultaneously. Each variant represents a unique \
combination of factor levels.

**Use case**: Testing combinations of thumbnail style + title format + \
position.

**Requirements**:
- Minimum 2 variants
- Each variant can carry a JSON payload with factor values

---

## Interleaving (INTERLEAVING)

Merge ranked lists from 2+ algorithms into a single interleaved list. \
Measures which algorithm users prefer based on engagement with items \
from each source.

**Use case**: Comparing recommendation or search ranking algorithms.

**Requirements**:
- `InterleavingConfig` specifying the algorithm (Team Draft, Optimized, \
or Multileave)
- Algorithm outputs provided at request time via `GetInterleavedList`

**Analysis**: Sign test + Bradley-Terry model

---

## Session-Level (SESSION_LEVEL)

Assigns variants per session rather than per user. The same user may see \
different variants across sessions.

**Use case**: Testing session-specific experiences (e.g., onboarding flows, \
content discovery paths).

**Requirements**:
- `SessionConfig` with `session_id_attribute`
- `min_sessions_per_user` for analysis filtering

**Analysis**: Clustered standard errors to account for within-user correlation

---

## Playback QoE (PLAYBACK_QOE)

Specialized for video streaming quality-of-experience experiments. M3 \
computes QoE-specific metrics (rebuffer rate, bitrate, startup time).

**Use case**: Testing CDN configurations, adaptive bitrate algorithms, player \
optimizations.

**Requirements**:
- At least one QoE guardrail metric
- QoE metrics defined in M3

**Analysis**: Cross-references QoE metrics with engagement metrics

---

## Multi-Armed Bandit (MAB)

Adaptive experiment where traffic allocation shifts toward better-performing \
arms over time using Thompson Sampling.

**Use case**: Optimizing content recommendations in real-time, promotional \
offers.

**Requirements**:
- `BanditConfig` with arm definitions
- Reward events flowing through M2

**Analysis**: IPW-adjusted analysis to correct for adaptive allocation bias

---

## Contextual Bandit (CONTEXTUAL_BANDIT)

Like MAB but uses user context features (e.g., genre preference, time of \
day) to personalize arm selection via LinUCB.

**Use case**: Personalized content recommendations, context-aware promotions.

**Requirements**:
- `BanditConfig` with `context_feature_keys`
- Context features in assignment request attributes

---

## Cumulative Holdout (CUMULATIVE_HOLDOUT)

A permanent baseline group that measures the total algorithmic lift over \
time. Never auto-concludes.

**Use case**: Measuring the cumulative impact of all recommendation \
improvements vs. a static baseline.

**Requirements**:
- `is_cumulative_holdout = true`
- M5 enforces no auto-conclusion
- Assignment Service prioritizes holdout assignment before layer allocation
"""

DEPLOYMENT_GUIDE = """\
# Deployment & Infrastructure

## Prerequisites

- **Rust** 1.80+ (for M1 Assignment, M4a Analysis, M4b Bandit)
- **Go** 1.22+ (for M5 Management, M3 Metrics, M7 Flags)
- **Node.js** 20+ (for M6 UI)
- **Docker** & Docker Compose
- **PostgreSQL** 16+
- **Apache Kafka** 3.6+
- **Redis** 7+

## Quick Start (Local Development)

```bash
# Clone the repository
git clone https://github.com/org/kaizen-experimentation.git
cd kaizen-experimentation

# Copy environment config
cp .env.example .env

# Start all infrastructure + services
just setup    # infra + codegen + deps + seed + test
```

The `just setup` command:
1. Starts PostgreSQL, Kafka, Redis, and Delta Lake via Docker Compose
2. Runs `buf generate` for Protobuf codegen
3. Installs Rust, Go, and Node.js dependencies
4. Runs database migrations (`sql/` directory)
5. Seeds test data
6. Runs the full test suite

## Service Endpoints

| Service | Default Port | Protocol |
|---------|-------------|----------|
| M1 Assignment | `:50051` | ConnectRPC |
| M2 Pipeline | `:50052` | ConnectRPC |
| M3 Metrics | `:50053` | ConnectRPC |
| M4a Analysis | `:50054` | ConnectRPC |
| M4b Bandit | `:50055` | ConnectRPC |
| M5 Management | `:50056` | ConnectRPC |
| M6 UI | `:3000` | HTTP |
| M7 Flags | `:50057` | ConnectRPC |

## Production Deployment Considerations

### Scaling

| Service | Scaling Strategy | Notes |
|---------|-----------------|-------|
| M1 Assignment | Horizontal (stateless) | Crash-only; any replica serves any request |
| M2 Pipeline | Horizontal + Kafka partitions | Scale consumers with partition count |
| M3 Metrics | Vertical (Spark) | Spark cluster sizing based on event volume |
| M4a Analysis | Horizontal (stateless) | CPU-bound; scale by concurrent analyses |
| M4b Bandit | **Single-threaded** (LMAX) | One active instance per policy; see ADR-002 |
| M5 Management | Horizontal | Standard CRUD service |
| M6 UI | CDN + SSR | Next.js with CDN for static assets |
| M7 Flags | Horizontal (stateless) | Similar to M1; uses CGo hash bridge |

### Database

- **PostgreSQL**: Primary data store for experiments, metrics, flags
- **Delta Lake**: Long-term event storage for historical analysis
- **RocksDB**: Bandit policy state (M4b) — crash-only, recoverable from \
snapshots (ADR-003)

### Monitoring

Key metrics to monitor:
- **Assignment latency** (M1): p50 < 5ms, p99 < 20ms
- **Event ingestion rate** (M2): Events/second throughput
- **Kafka consumer lag**: All consumer groups should have near-zero lag
- **Guardrail breach rate**: Alerts from M3 to M5
- **Bandit policy update frequency** (M4b): Should match reward event rate
"""

SECURITY_AUTH = """\
# Security & Authentication

## API Authentication

All Kaizen APIs require authentication via bearer tokens:

```
Authorization: Bearer <token>
```

Tokens are issued by your identity provider and validated by each service.

## Role-Based Access Control

The Dashboard UI (M6) enforces role-based access:

| Role | Permissions |
|------|------------|
| **Viewer** | View experiments, results, and dashboards |
| **Experimenter** | Create/edit experiments, start/pause/conclude |
| **Admin** | All permissions + manage users, layers, metric definitions |

## Data Security

- **No PII in events**: User IDs should be opaque identifiers, not emails
- **Payload encryption**: Variant payloads can contain encrypted data
- **Audit trail**: All lifecycle transitions logged with actor and timestamp
- **Guardrail auto-pause**: Experiments breaching safety thresholds are \
automatically paused (ADR-008)

## Network Security

- All service-to-service communication uses ConnectRPC with TLS
- SDK-to-service communication should use HTTPS in production
- Kafka communication should use SASL/SSL
- Database connections should use SSL

## Hash Determinism & Privacy

MurmurHash3 bucketing is deterministic but not reversible — you cannot \
derive the user ID from the bucket assignment. The hash uses a per-experiment \
salt, so bucket assignments differ across experiments.
"""


# =============================================================================
# USER EXPERIENCE GUIDE CONTENT
# =============================================================================

UX_OVERVIEW = """\
# User Experience Guide — Kaizen Dashboard

This guide walks through the Kaizen Experimentation Dashboard from a human \
user's perspective. Whether you're a product manager creating your first \
experiment or a data scientist analyzing results, this guide covers the \
complete workflow.

## Dashboard Overview

The Kaizen Dashboard (M6) is a Next.js application that provides a visual \
interface for managing experiments, viewing results, and monitoring the health \
of your experimentation program.

### Navigation Structure

```
Dashboard
+-- Experiments List (Home)
|   +-- Filters & Sorting
|   +-- Experiment Cards
+-- Create New Experiment
|   +-- Basic Information
|   +-- Metrics Configuration
|   +-- Variant Setup
|   +-- Guardrails
|   +-- Sequential Testing
|   +-- Targeting Rules
+-- Experiment Detail View
|   +-- Status & Lifecycle Actions
|   +-- Results Summary
|   +-- Treatment Effects Table
|   +-- Statistical Analysis Tabs
|   |   +-- CATE (Conditional Average Treatment Effect)
|   |   +-- Guardrail Monitoring
|   |   +-- Novelty Detection
|   |   +-- Interference Check
|   |   +-- QoE Metrics
|   |   +-- Interleaving Results
|   |   +-- Session-Level Analysis
|   |   +-- Surrogate Models
|   |   +-- Holdout Analysis
|   +-- SRM Banner (Sample Ratio Mismatch)
|   +-- Query Log
+-- Settings
    +-- Layer Allocation Chart
```

## Key UI Components

| Component | Purpose |
|-----------|---------|
| **Experiment Card** | Summary row showing name, owner, type, state, date |
| **State Badge** | Color-coded badge showing experiment state |
| **Type Badge** | Badge showing experiment type (A/B, MAB, etc.) |
| **SRM Banner** | Warning banner for Sample Ratio Mismatch |
| **Starting Checklist** | Progress during STARTING transitional state |
| **Concluding Progress** | Progress during CONCLUDING transitional state |
| **Results Summary** | High-level results with confidence intervals |
| **Treatment Effects Table** | Detailed treatment effects for each metric |
"""

UX_CREATING_EXPERIMENT = """\
# Creating an Experiment

This guide walks through creating a new experiment in the Kaizen Dashboard \
step by step.

## Step 1: Navigate to Create

From the Experiments list page, click the **"New Experiment"** button in the \
top-right corner.

> **Note**: You need the **Experimenter** role or higher to create experiments. \
Viewers will see the button grayed out.

## Step 2: Basic Information

Fill in the core experiment details:

| Field | Required | Description | Example |
|-------|----------|-------------|---------|
| **Name** | Yes | Unique experiment identifier | `homepage_recs_v3` |
| **Owner Email** | Yes | Experiment owner's email | `alice@streamco.com` |
| **Description** | No | Hypothesis being tested | "Test CF v2 vs. current" |
| **Experiment Type** | Yes | Type of experiment | A/B Test |
| **Layer ID** | Yes | Traffic layer for isolation | `layer-homepage` |

### Choosing an Experiment Type

The type dropdown offers 8 options:

- **A/B Test** — Standard randomized trial (most common)
- **Multivariate** — Test multiple factor combinations
- **Interleaving** — Compare ranking algorithms
- **Session-Level** — Per-session randomization
- **Playback QoE** — Video streaming quality experiments
- **Multi-Armed Bandit** — Adaptive traffic allocation
- **Contextual Bandit** — Personalized adaptive allocation
- **Cumulative Holdout** — Permanent baseline measurement

### Understanding Layers

Layers control traffic isolation. Experiments in the **same layer** share \
traffic (mutually exclusive), while experiments in **different layers** \
are orthogonal (users can be in both).

**Example**: If you have two homepage experiments, put them in the same \
layer (`layer-homepage`) to prevent interference.

## Step 3: Metrics Configuration

| Field | Required | Description |
|-------|----------|-------------|
| **Primary Metric** | Yes | The main metric you're optimizing |
| **Secondary Metrics** | No | Additional metrics to monitor (comma-separated) |

**Tips**:
- Choose a primary metric that directly measures your hypothesis
- Add secondary metrics for broader impact assessment
- Metric IDs must match definitions in the Metrics Engine (M3)

## Step 4: Variant Setup

By default, two variants are created: **control** (50%) and **treatment** \
(50%).

For each variant, configure:

| Field | Description |
|-------|-------------|
| **Name** | Variant identifier (e.g., "control", "treatment-a") |
| **Traffic** | Fraction of traffic (0.0 to 1.0) |
| **Control** | Radio button — exactly one variant must be control |
| **Payload JSON** | Optional JSON payload delivered to the SDK |

**Rules**:
- Traffic fractions must sum to exactly 1.0 (shown in green when valid)
- Minimum 2 variants for A/B tests
- Exactly one variant must be marked as control
- Payload JSON must be valid JSON

**Example payload**:
```json
{
  "algorithm": "collaborative_filtering_v2",
  "num_recommendations": 20
}
```

## Step 5: Guardrails

Guardrails are safety metrics that automatically pause your experiment if \
breached.

Click **"Add Guardrail"** to add a guardrail metric:

| Field | Description |
|-------|-------------|
| **Metric ID** | The guardrail metric to monitor |
| **Threshold** | Breach threshold value |
| **Breaches** | Consecutive breaches required before action (default: 1) |

**Action on Breach**:
- **Auto-Pause** (default, recommended): Experiment is automatically paused
- **Alert Only**: Owner is alerted but experiment continues

> **Best Practice**: Always set at least one guardrail for user-facing \
experiments. Common guardrails include error rate, latency, and rebuffer rate.

## Step 6: Sequential Testing (Optional)

Enable sequential testing to peek at results before full sample size.

| Field | Description |
|-------|-------------|
| **Method** | mSPRT (flexible) or GST with O'Brien-Fleming/Pocock |
| **Planned Looks** | Number of analysis looks (GST only, minimum 2) |
| **Overall Alpha** | Total Type I error budget (default: 0.05) |

**When to use**:
- **mSPRT**: Check results at any time (lower power but maximum flexibility)
- **GST O'Brien-Fleming**: Pre-committed schedule (higher power, conservative)
- **GST Pocock**: Equal stopping probability at each look

## Step 7: Submit

Click **"Create Experiment"** to create the experiment in DRAFT state. From \
there:
1. Review the configuration
2. Click **"Start"** to begin the STARTING validation process
3. Once validation passes, the experiment transitions to RUNNING
"""

UX_EXPERIMENT_LIFECYCLE = """\
# Experiment Lifecycle

Every experiment in Kaizen follows a well-defined lifecycle with both stable \
and transitional states. Understanding this lifecycle is key to managing \
experiments effectively.

## State Diagram

```
                    +----------+
                    |  DRAFT   |
                    +----+-----+
                         | Start
                    +----v-----+
                    | STARTING |  (transitional)
                    | - Validate config
                    | - Confirm metrics
                    | - Warm bandit policy
                    | - Check power
                    +----+-----+
                         | Validation passes
                    +----v-----+
              +-----|  RUNNING |-----+
              |     +----+-----+     |
              |          |           |
         Pause|     Conclude    Auto-pause
              |          |      (guardrail)
              v          |           |
        +---------+      |     +-----v----+
        | PAUSED  |------+     |  PAUSED  |
        +----+----+ Resume     |  (auto)  |
             |                 +----------+
             |
        +----v------+
        |CONCLUDING |  (transitional)
        | - Final analysis
        | - Policy snapshots
        | - Surrogate projections
        +----+------+
             |
        +----v------+
        | CONCLUDED |
        +----+------+
             | Archive
        +----v------+
        | ARCHIVED  |
        +-----------+
```

## State Details

### DRAFT
- **What happens**: Experiment is configured but not yet validated
- **What you can do**: Edit all fields, add/remove variants, change metrics
- **Next action**: Click "Start" to begin validation

### STARTING (Transitional)
- **What happens**: Platform validates the experiment configuration
- **UI shows**: A checklist of validation steps with progress indicators
- **Validation checks**:
  - Config completeness (all required fields present)
  - Metric availability (primary + secondary metrics exist in M3)
  - Layer allocation (sufficient bucket space available)
  - Power analysis (sample size adequate for desired effect size)
  - Bandit policy warm-up (for MAB/contextual bandit types)
- **Duration**: Typically 5-30 seconds
- **If validation fails**: Returns to DRAFT with error messages

### RUNNING
- **What happens**: Actively collecting data and serving assignments
- **What you can do**: View real-time results, pause, conclude
- **For bandits**: Policy is actively adapting based on reward signals
- **Guardrails**: Monitored continuously; auto-pause on breach

### PAUSED
- **What happens**: Traffic allocation drops to 0%; no new assignments
- **Why it happens**: Manual pause or automatic guardrail breach
- **What you can do**: Investigate the issue, then resume or conclude
- **Auto-pause**: Shows the guardrail metric that triggered the pause

### CONCLUDING (Transitional)
- **What happens**: Platform runs final analysis
- **UI shows**: Progress indicator with analysis steps
- **Analysis steps**:
  - Final statistical tests (t-test, mSPRT, GST)
  - Policy snapshot for bandits
  - Surrogate model projections
  - IPW estimates for adaptive experiments
- **Duration**: 30 seconds to 5 minutes depending on data volume
- **Important**: Result queries return 503 during this state

### CONCLUDED
- **What happens**: Analysis complete, results available
- **What you can do**: View final results, decide to ship or revert, archive
- **Results**: Fully available with confidence intervals, p-values

### ARCHIVED
- **What happens**: Experiment retained for historical reference
- **Results**: Still queryable for retrospective analysis
- **Bucket space**: Released back to the layer for reuse (with cooldown)
"""

UX_READING_RESULTS = """\
# Reading Experiment Results

Once an experiment is RUNNING or CONCLUDED, the detail view shows \
comprehensive results. This guide explains how to interpret each section.

## Results Summary

The top-level results summary shows:

- **Primary metric**: Effect size with confidence interval
- **Statistical significance**: Whether the result is significant
- **Recommendation**: Ship / Don't Ship / Inconclusive
- **Sample size**: Number of users in each variant

## SRM Banner

If a **Sample Ratio Mismatch** is detected, a prominent warning banner \
appears at the top of the results page.

**What is SRM?** When the observed traffic split differs significantly from \
the configured split (e.g., you configured 50/50 but observe 52/48), it \
indicates a bug in the assignment or logging pipeline.

**What to do**: Investigate the root cause before trusting any results. \
Common causes:
- Bot traffic affecting one variant more than another
- Logging bugs dropping events for one variant
- Redirect-based experiments where one variant loads faster

## Treatment Effects Table

Shows the effect of each treatment variant compared to control:

| Column | Description |
|--------|-------------|
| **Metric** | Metric name |
| **Control Mean** | Average value in control group |
| **Treatment Mean** | Average value in treatment group |
| **Absolute Diff** | Treatment - Control |
| **Relative Diff** | (Treatment - Control) / Control as percentage |
| **CI Lower** | Lower bound of 95% confidence interval |
| **CI Upper** | Upper bound of 95% confidence interval |
| **p-value** | Statistical significance (< 0.05 = significant) |

## Analysis Tabs

### CATE Tab (Conditional Average Treatment Effect)
Shows how treatment effects vary across user segments. Useful for \
identifying which user groups benefit most from the treatment.

### Guardrail Tab
Displays guardrail metric values over time with threshold lines. Shows \
whether any guardrail was breached and when.

### Novelty Tab
Detects novelty effects — initial spikes in engagement that fade over \
time. Important for distinguishing genuine improvements from curiosity.

### Interference Tab
Checks for network effects and interference between users. Important \
when users can influence each other (e.g., social features).

### QoE Tab (Playback QoE experiments)
Shows video streaming quality metrics: rebuffer rate, bitrate, startup \
time, and their correlation with engagement metrics.

### Interleaving Tab (Interleaving experiments)
Shows algorithm preference results from the sign test and Bradley-Terry \
model. Indicates which algorithm users prefer.

### Session-Level Tab (Session-level experiments)
Shows results with clustered standard errors accounting for within-user \
correlation across sessions.

### Surrogate Tab
Shows surrogate model predictions and calibration metrics. Useful for \
early stopping decisions when the surrogate model has high fidelity.

### Holdout Tab (Cumulative holdout experiments)
Shows the cumulative algorithmic lift over time compared to the permanent \
baseline group.

## CUPED Toggle

The **CUPED** (Controlled-experiment Using Pre-Experiment Data) toggle \
enables variance reduction using pre-experiment data. When enabled:

- Confidence intervals become narrower
- You can detect smaller effects with the same sample size
- Results are adjusted for pre-experiment user behavior

**Recommendation**: Enable CUPED when pre-experiment data is available \
(typically after 1+ weeks of data collection).

## Query Log

The Query Log table shows all SQL queries executed by the Metrics Engine \
(M3) for this experiment. Useful for debugging metric computation issues.
"""

UX_MANAGING_EXPERIMENTS = """\
# Managing Experiments Day-to-Day

## Experiments List Page

The home page shows all experiments in a sortable, filterable table.

### Filtering

Use the filter toolbar to narrow down experiments:

| Filter | Options |
|--------|---------|
| **State** | Draft, Starting, Running, Paused, Concluding, Concluded, Archived |
| **Type** | A/B, Multivariate, Interleaving, Session-Level, QoE, MAB, etc. |
| **Owner** | Filter by owner email |
| **Search** | Free-text search on experiment name |

### Sorting

Click any column header to sort:
- **Name** (alphabetical)
- **Type** (by experiment type)
- **State** (by lifecycle state)
- **Created** (by creation date)

Click again to toggle ascending/descending.

## Experiment Actions

Available actions depend on the current state:

| State | Available Actions |
|-------|------------------|
| **Draft** | Edit, Start, Delete |
| **Starting** | (wait for validation) |
| **Running** | Pause, Conclude |
| **Paused** | Resume, Conclude |
| **Concluding** | (wait for analysis) |
| **Concluded** | Archive |
| **Archived** | (read-only) |

## Common Workflows

### 1. Standard A/B Test

```
Create (DRAFT) -> Start -> Monitor (RUNNING) -> Conclude -> Ship or Revert
```

**Timeline**: Typically 1-4 weeks depending on traffic and effect size.

### 2. Guardrail-Triggered Investigation

```
RUNNING -> Auto-Pause (guardrail breach) -> Investigate -> Resume or Conclude
```

**What to check**:
1. Which guardrail metric breached?
2. Is the breach real or a false alarm?
3. Check the Guardrail tab for time series
4. If real: conclude and revert. If false alarm: resume.

### 3. Sequential Testing Workflow

```
Create with sequential config -> Start -> Peek periodically -> Stop if significant
```

**Key rule**: Only stop early if the sequential test says you can. Looking \
at raw p-values and stopping early inflates your false positive rate.

### 4. Bandit Experiment

```
Create (MAB/Contextual) -> Start -> Monitor adaptation -> Conclude
```

**Key differences from A/B**:
- Traffic allocation shifts automatically toward better arms
- Results use IPW-adjusted analysis to correct for adaptive allocation
- Reward events must flow through M2 for policy learning

### 5. Feature Flag to Experiment Promotion

```
Create flag -> Gradual rollout -> Promote to experiment -> Analyze -> Ship
```

## Best Practices

1. **Always set guardrails** for user-facing experiments
2. **Use CUPED** when pre-experiment data is available
3. **Don't peek at raw p-values** — use sequential testing to peek safely
4. **Document your hypothesis** in the experiment description
5. **Archive concluded experiments** to keep the list clean
6. **Use layers** to prevent experiment interference
7. **Check for SRM** before trusting results
8. **Wait for sufficient sample size** before concluding
"""

UX_DASHBOARD_TIPS = """\
# Dashboard Tips & Troubleshooting

## Understanding State Badges

| Badge Color | State | Meaning |
|-------------|-------|---------|
| Gray | Draft | Not yet started |
| Blue | Starting | Validating configuration |
| Green | Running | Actively collecting data |
| Yellow | Paused | Traffic stopped |
| Blue | Concluding | Running final analysis |
| Purple | Concluded | Results available |
| Gray | Archived | Historical reference |

## Common Issues & Solutions

### "Starting" state stuck for more than 5 minutes
- **Cause**: Metric definitions not found in M3, or layer has no buckets
- **Fix**: Check that all metric IDs exist and the layer has free space

### SRM detected
- **Cause**: Assignment or logging bug
- **Fix**: Do NOT trust results. Investigate logging pipeline, check for \
bot traffic, verify SDK integration

### Guardrail auto-paused my experiment
- **Cause**: A guardrail metric breached its threshold
- **Fix**: Check the Guardrail tab. If it's a real issue, conclude and \
revert. If noise, increase `consecutiveBreachesRequired` and resume.

### Results show "Inconclusive"
- **Cause**: Not enough data to detect the effect size
- **Fix**: Wait for more data, or use CUPED to reduce variance. Consider \
whether the expected effect size is realistic.

### Bandit not adapting
- **Cause**: Reward events not flowing to M4b
- **Fix**: Verify reward events are being sent through M2 and reaching \
the `reward_events` Kafka topic

### "Concluding" state stuck for more than 10 minutes
- **Cause**: Large data volume or M4a analysis backlog
- **Fix**: Check M4a service health. For very large experiments, analysis \
can take up to 30 minutes.

## Performance Tips

- **Bulk-fetch assignments** on app startup using `GetAssignments` instead \
of individual `GetAssignment` calls
- **Cache assignments client-side** for the session duration to avoid \
repeated network calls
- **Use LocalProvider as fallback** for zero-latency assignments when the \
remote service is unavailable
- **Batch events** to reduce network overhead in the event pipeline
"""


def get_integration_pages():
    """Return list of (title, content) tuples for Integration Guide."""
    return [
        ("Integration Overview", INTEGRATION_OVERVIEW),
        ("SDK Overview", SDK_OVERVIEW),
        ("Web SDK (TypeScript)", WEB_SDK),
        ("Server Go SDK", SERVER_GO_SDK),
        ("Server Python SDK", SERVER_PYTHON_SDK),
        ("Mobile SDKs (iOS & Android)", MOBILE_SDKS),
        ("API Reference", API_REFERENCE),
        ("Event Pipeline Integration", EVENT_PIPELINE),
        ("Feature Flags", FEATURE_FLAGS),
        ("Experiment Types", EXPERIMENT_TYPES_DOC),
        ("Deployment & Infrastructure", DEPLOYMENT_GUIDE),
        ("Security & Authentication", SECURITY_AUTH),
    ]


def get_ux_pages():
    """Return list of (title, content) tuples for User Experience Guide."""
    return [
        ("Dashboard Overview", UX_OVERVIEW),
        ("Creating an Experiment", UX_CREATING_EXPERIMENT),
        ("Experiment Lifecycle", UX_EXPERIMENT_LIFECYCLE),
        ("Reading Experiment Results", UX_READING_RESULTS),
        ("Managing Experiments Day-to-Day", UX_MANAGING_EXPERIMENTS),
        ("Dashboard Tips & Troubleshooting", UX_DASHBOARD_TIPS),
    ]
