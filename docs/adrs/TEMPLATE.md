# ADR-NNN: [Short Title]

**Status**: Proposed <!-- Proposed | Accepted | Accepted (In Progress) | Accepted (Planned — Sprint X.Y) | Accepted and Implemented | Deprecated | Superseded -->
**Date**: YYYY-MM-DD
**Deciders**: Agent-N ([Module or role])
**Cluster**: [A–F] — [Cluster name, if applicable]

---

## Context

What gap, requirement, or problem motivates this decision? Ground in research and prior art where applicable.

---

## Decision

What is the change we're proposing? Break into subsections by topic (design, schema, integration) where useful.

---

## Consequences

### Benefits

1. What becomes easier or possible as a result?

### Trade-offs

1. What becomes more difficult, expensive, or constrained?

---

## Implementation Details

### Proto Schema

```protobuf
// Schema changes, if any
```

### Crate Layout / Public API

Describe the module structure and key types.

### Integration

How this interacts with existing modules (M1, M4b, etc.).

---

## Validation

### Unit Tests / Proptest Invariants

- Key correctness properties to verify.

### Golden-File Tests

- Reference implementation and precision target (e.g., R `TOSTER` package to 6 decimals).

### Integration / Contract Tests

- Cross-module behavior.

---

## Dependencies

- **ADR-XXX** ([Short name]): [relationship]
- **Enables ADR-YYY**: [what this unlocks]

---

## Rejected Alternatives

| Alternative | Reason Rejected |
|-------------|-----------------|
|             |                 |

---

## References

- Citations to papers, blog posts, prior art
- Links to other ADRs, design docs, or issues
