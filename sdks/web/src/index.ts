/**
 * Experimentation Platform — Web SDK
 *
 * Implements the Provider Abstraction pattern (ADR-007) with three backends:
 *   - RemoteProvider: Fetches assignments from the Assignment Service via ConnectRPC
 *   - LocalProvider:  Evaluates assignments locally using cached config
 *   - MockProvider:   Returns deterministic assignments for testing
 *
 * Usage:
 *   import { ExperimentClient, RemoteProvider } from '@experimentation/sdk-web';
 *
 *   const client = new ExperimentClient({
 *     provider: new RemoteProvider({ baseUrl: 'https://assignment.example.com' }),
 *     userId: 'user-123',
 *   });
 *
 *   const variant = await client.getVariant('homepage_recs_v2');
 */

// ---------------------------------------------------------------------------
// Core Types
// ---------------------------------------------------------------------------

/** A variant assignment for a single experiment. */
export interface Assignment {
  experimentId: string;
  variantName: string;
  /** Opaque payload from the variant configuration. */
  payload: Record<string, unknown>;
  /** Whether this assignment was served from cache. */
  fromCache: boolean;
}

/** User attributes for targeting evaluation. */
export interface UserAttributes {
  userId: string;
  [key: string]: string | number | boolean | string[];
}

/** Configuration for an experiment (used by LocalProvider). */
export interface ExperimentConfig {
  experimentId: string;
  hashSalt: string;
  layerName: string;
  variants: Array<{
    name: string;
    trafficFraction: number;
    isControl: boolean;
    payload: Record<string, unknown>;
  }>;
  allocationStart: number;
  allocationEnd: number;
  totalBuckets: number;
}

// ---------------------------------------------------------------------------
// Provider Interface
// ---------------------------------------------------------------------------

/**
 * Provider abstraction — all assignment backends implement this interface.
 * See ADR-007 for the design rationale.
 */
export interface AssignmentProvider {
  /** Initialize the provider (fetch config, establish connection, etc.) */
  initialize(): Promise<void>;

  /** Get a variant assignment for the given experiment and user. */
  getAssignment(
    experimentId: string,
    attributes: UserAttributes,
  ): Promise<Assignment | null>;

  /** Get assignments for all active experiments for the given user. */
  getAllAssignments(
    attributes: UserAttributes,
  ): Promise<Map<string, Assignment>>;

  /** Shut down the provider (close connections, flush logs, etc.) */
  destroy(): Promise<void>;
}

// ---------------------------------------------------------------------------
// RemoteProvider
// ---------------------------------------------------------------------------

export interface RemoteProviderConfig {
  /** Base URL of the Assignment Service (e.g. 'https://assignment.example.com') */
  baseUrl: string;
  /** Request timeout in milliseconds (default: 2000) */
  timeoutMs?: number;
  /** Retry count on transient failures (default: 1) */
  retries?: number;
}

export class RemoteProvider implements AssignmentProvider {
  private config: RemoteProviderConfig;

  constructor(config: RemoteProviderConfig) {
    this.config = { timeoutMs: 2000, retries: 1, ...config };
  }

  async initialize(): Promise<void> {
    // TODO (Agent-1): Establish ConnectRPC transport to Assignment Service
  }

  async getAssignment(
    experimentId: string,
    attributes: UserAttributes,
  ): Promise<Assignment | null> {
    // TODO (Agent-1): Call AssignmentService.GetAssignment via ConnectRPC
    void experimentId;
    void attributes;
    return null;
  }

  async getAllAssignments(
    attributes: UserAttributes,
  ): Promise<Map<string, Assignment>> {
    // TODO (Agent-1): Call AssignmentService.GetAllAssignments via ConnectRPC
    void attributes;
    return new Map();
  }

  async destroy(): Promise<void> {
    // TODO (Agent-1): Close transport
  }
}

// ---------------------------------------------------------------------------
// LocalProvider
// ---------------------------------------------------------------------------

export interface LocalProviderConfig {
  /** Static experiment configs for local evaluation. */
  experiments: ExperimentConfig[];
}

export class LocalProvider implements AssignmentProvider {
  private experiments: Map<string, ExperimentConfig> = new Map();

  constructor(config: LocalProviderConfig) {
    for (const exp of config.experiments) {
      this.experiments.set(exp.experimentId, exp);
    }
  }

  async initialize(): Promise<void> {
    // No-op for static config
  }

  async getAssignment(
    experimentId: string,
    attributes: UserAttributes,
  ): Promise<Assignment | null> {
    const config = this.experiments.get(experimentId);
    if (!config) return null;

    // TODO (Agent-1): Implement MurmurHash3 bucket assignment in WASM
    //   1. hash = murmur3(`${attributes.userId}:${config.hashSalt}`)
    //   2. bucket = hash % config.totalBuckets
    //   3. if bucket < config.allocationStart || bucket > config.allocationEnd → null
    //   4. map bucket to variant by cumulative traffic fractions
    void attributes;
    void config;
    return null;
  }

  async getAllAssignments(
    attributes: UserAttributes,
  ): Promise<Map<string, Assignment>> {
    const results = new Map<string, Assignment>();
    for (const experimentId of this.experiments.keys()) {
      const assignment = await this.getAssignment(experimentId, attributes);
      if (assignment) results.set(experimentId, assignment);
    }
    return results;
  }

  async destroy(): Promise<void> {
    this.experiments.clear();
  }
}

// ---------------------------------------------------------------------------
// MockProvider (for testing)
// ---------------------------------------------------------------------------

export interface MockAssignment {
  experimentId: string;
  variantName: string;
  payload?: Record<string, unknown>;
}

export class MockProvider implements AssignmentProvider {
  private assignments: Map<string, MockAssignment> = new Map();

  constructor(assignments: MockAssignment[] = []) {
    for (const a of assignments) {
      this.assignments.set(a.experimentId, a);
    }
  }

  async initialize(): Promise<void> {}

  async getAssignment(
    experimentId: string,
    _attributes: UserAttributes,
  ): Promise<Assignment | null> {
    const mock = this.assignments.get(experimentId);
    if (!mock) return null;
    return {
      experimentId: mock.experimentId,
      variantName: mock.variantName,
      payload: mock.payload ?? {},
      fromCache: false,
    };
  }

  async getAllAssignments(
    attributes: UserAttributes,
  ): Promise<Map<string, Assignment>> {
    const results = new Map<string, Assignment>();
    for (const [id] of this.assignments) {
      const a = await this.getAssignment(id, attributes);
      if (a) results.set(id, a);
    }
    return results;
  }

  /** Override an assignment at runtime (useful in tests). */
  setAssignment(experimentId: string, variantName: string, payload?: Record<string, unknown>): void {
    this.assignments.set(experimentId, { experimentId, variantName, payload });
  }

  async destroy(): Promise<void> {
    this.assignments.clear();
  }
}

// ---------------------------------------------------------------------------
// ExperimentClient
// ---------------------------------------------------------------------------

export interface ExperimentClientConfig {
  provider: AssignmentProvider;
  userId: string;
  attributes?: Record<string, string | number | boolean | string[]>;
  /** Fallback provider if primary fails (ADR-007 fallback chain). */
  fallbackProvider?: AssignmentProvider;
}

export class ExperimentClient {
  private provider: AssignmentProvider;
  private fallback?: AssignmentProvider;
  private attributes: UserAttributes;
  private initialized = false;

  constructor(config: ExperimentClientConfig) {
    this.provider = config.provider;
    this.fallback = config.fallbackProvider;
    this.attributes = { userId: config.userId, ...config.attributes };
  }

  async initialize(): Promise<void> {
    await this.provider.initialize();
    if (this.fallback) await this.fallback.initialize();
    this.initialized = true;
  }

  async getVariant(experimentId: string): Promise<string | null> {
    if (!this.initialized) await this.initialize();

    try {
      const assignment = await this.provider.getAssignment(experimentId, this.attributes);
      if (assignment) return assignment.variantName;
    } catch (err) {
      if (this.fallback) {
        const fallbackAssignment = await this.fallback.getAssignment(experimentId, this.attributes);
        if (fallbackAssignment) return fallbackAssignment.variantName;
      }
    }
    return null;
  }

  async getAssignment(experimentId: string): Promise<Assignment | null> {
    if (!this.initialized) await this.initialize();

    try {
      return await this.provider.getAssignment(experimentId, this.attributes);
    } catch (err) {
      if (this.fallback) {
        return await this.fallback.getAssignment(experimentId, this.attributes);
      }
    }
    return null;
  }

  async destroy(): Promise<void> {
    await this.provider.destroy();
    if (this.fallback) await this.fallback.destroy();
    this.initialized = false;
  }
}
