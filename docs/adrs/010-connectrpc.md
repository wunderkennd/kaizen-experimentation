# ADR-010: ConnectRPC as the RPC Framework

**Status**: Accepted
**Date**: 2026-03-03

---

## Context
We need an RPC framework that works across Go (M3, M5, M7), Rust (M1, M2, M4a, M4b), and TypeScript (M6 UI). The framework must support streaming (M1 config updates), unary RPCs (most operations), and browser clients (M6 dashboard).

## Decision
Use ConnectRPC for Go services (connect-go) and tonic for Rust services. Both use the same Protobuf schema. The M6 TypeScript UI uses @connectrpc/connect-web to call Go services directly from the browser. Rust services expose tonic-web for browser compatibility where needed.

## Alternatives Considered
- **gRPC everywhere**: Standard choice, but gRPC-Web requires a proxy (Envoy) for browser clients. ConnectRPC's HTTP/1.1 JSON mode eliminates this requirement for Go services.
- **REST/OpenAPI**: More familiar to frontend developers, but loses the schema-first proto contract. Manual serialization/deserialization code. No streaming support.
- **tRPC**: TypeScript-native, excellent DX for full-stack TypeScript. But our backend is Go + Rust, not TypeScript. tRPC doesn't support non-TypeScript backends.
- **gRPC for Rust, ConnectRPC for Go, REST for UI**: Mixed protocols increase integration complexity. Using ConnectRPC + tonic with shared protos provides a single contract layer.

## Consequences
- Go services use connect-go interceptors for auth, tracing, and metrics.
- Rust services use tonic interceptors for the same concerns.
- M6 UI calls Go services via ConnectRPC (no proxy needed) and Rust services via tonic-web (if direct browser access is needed; most Rust service access is Go-to-Rust server-side).
- buf toolchain generates Go, Rust (via tonic-build), and TypeScript clients from the same proto source.
