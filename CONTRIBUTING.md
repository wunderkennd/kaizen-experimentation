# Contributing to the Experimentation Platform

This guide covers the conventions and workflows that keep our multi-agent
development model coherent. Every agent should read this before their first PR.

## Quick Start

```bash
git clone <repo-url> && cd experimentation-platform
cp .env.example .env
just setup    # Starts infra, generates code, installs deps, seeds data, runs tests
```

## Branch Naming

All branches follow the pattern:

```
<agent>/<type>/<short-description>
```

Examples:

```
agent-1/feat/wasm-hash-binding
agent-4/fix/cuped-negative-variance
agent-5/refactor/experiment-crud-validation
agent-6/chore/upgrade-recharts
```

Types: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`, `perf`

The `main` branch is protected. All changes land via pull request.

## Commit Messages

We use [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <description>

[optional body]

[optional footer(s)]
```

Scope is the module number or crate name:

```
feat(m1): add WASM hash binding with 10K vector validation
fix(experimentation-stats): handle zero-variance arms in CUPED
test(m5): add integration tests for experiment state transitions
docs(adr-011): document decision on metric aggregation strategy
```

Breaking changes use a `!` suffix: `feat(m1)!: change bucket() return type to u32`

## Pull Request Process

1. **Create your branch** from `main` using the naming convention above.
2. **Keep PRs focused.** One logical change per PR. If a feature touches
   multiple modules, coordinate via the contract versioning protocol below.
3. **Fill in the PR template** (auto-populated when you open a PR).
4. **All CI checks must pass** before merge: lint, test, hash parity, type check.
5. **One approving review** required. Cross-module PRs require review from the
   affected module's agent.
6. **Squash merge** to `main`. The squash commit message should follow
   conventional commit format.

### PR Template

```markdown
## Summary
<!-- What does this PR do? Which module(s) does it affect? -->

## Agent
<!-- e.g. Agent-1 (Assignment Service) -->

## Type
<!-- feat | fix | refactor | test | docs | chore | perf -->

## Related Issues
<!-- Link to issues or ADRs -->

## Contract Changes
<!-- If this PR changes a proto schema, API contract, or shared crate interface:
     - Which contract version is bumped?
     - Have you notified downstream agents?
     - Is there a migration path? -->

## Testing
<!-- How was this tested? Include relevant commands. -->
- [ ] Unit tests pass (`just test-rust` / `just test-go` / `just test-ts`)
- [ ] Integration tests pass (if applicable)
- [ ] Hash parity validated (if touching hash/assignment)
- [ ] Benchmarks run (if performance-sensitive)

## Checklist
- [ ] Code follows project conventions
- [ ] Proto changes are backward-compatible (or ADR documents the break)
- [ ] Documentation updated (if user-facing)
- [ ] No new warnings from linters
```

## Contract Versioning Protocol

When an agent changes a shared interface (proto schema, Rust crate public API,
Go package exported types), the following protocol applies:

### Proto Schema Changes

1. **Additive changes** (new fields, new RPCs, new messages): No version bump
   needed. Add fields with new tag numbers. Downstream agents pick up changes
   on their next `just codegen`.

2. **Breaking changes** (removed fields, renamed messages, changed semantics):
   Requires a new ADR documenting the rationale. The agent proposing the break
   must open a coordination PR that:
   - Updates the proto file
   - Updates all call sites they own
   - Lists affected downstream agents in the PR description
   - Tags downstream agents for review

3. **buf breaking** runs in CI against the `main` branch. Breaking changes that
   haven't gone through the protocol will fail CI.

### Rust Crate API Changes

Shared crates (`experimentation-core`, `experimentation-hash`, `experimentation-proto`)
follow the same additive/breaking distinction:

- **Additive**: New public functions, new enum variants (if `#[non_exhaustive]`),
  new optional fields. No coordination required.
- **Breaking**: Removed or renamed public items, changed signatures, changed
  behavior. Requires cross-agent PR review.

The workspace `Cargo.toml` pins all internal crate versions. Bumps to shared
crates should be called out in the PR description.

### Notification Protocol

For any contract change, the authoring agent must:

1. Post in the coordination channel (Slack/Teams/GitHub Discussion) with:
   - Which contract changed
   - Summary of the change
   - Migration steps for downstream agents
   - Timeline (when will the old contract stop working)
2. Tag affected agents in the PR

## Code Style

### Rust

- Follow `rustfmt` defaults (enforced by CI)
- `clippy --all-features -- -D warnings` must pass
- Use `experimentation_core::Error` for all error types
- Use `assert_finite!()` for any floating-point computation
- Prefer `thiserror` for library crates, `anyhow` only in binary crates

### Go

- Follow `gofmt` and `go vet` (enforced by CI)
- Use `connectrpc.com/connect` for all RPC handlers
- Structured logging via `slog` (JSON format in production, text in dev)
- Context propagation: always pass `context.Context` as first parameter

### TypeScript

- ESLint + Prettier (enforced by CI)
- Strict TypeScript (`"strict": true` in tsconfig)
- Use `@connectrpc/connect-web` for API calls
- Components in `src/components/`, pages in `src/app/`

## Testing Conventions

### Unit Tests

- Rust: `#[cfg(test)] mod tests` in each module, plus `tests/` for integration
- Go: `_test.go` files alongside source, `testify/assert` for assertions
- TypeScript: `vitest` with files in `src/__tests__/`

### Golden Files

Statistical tests use golden files in `crates/experimentation-stats/tests/golden/`.
To update golden files after an intentional algorithm change:

```bash
UPDATE_GOLDEN=1 cargo test --workspace
```

### Property-Based Tests

Rust crates use `proptest` for invariant testing. Nightly CI runs extended
proptest campaigns (10,000 cases, 30-minute timeout).

### Integration Tests

Go integration tests use the `//go:build integration` build tag and run against
`docker-compose.test.yml`:

```bash
just test-integration
```

## Adding a New ADR

1. Copy the template: `cp adrs/TEMPLATE.md adrs/NNN-short-title.md`
2. Fill in Status, Context, Decision, Consequences
3. Open a PR with the ADR for team review
4. Once approved, update the ADR status to "Accepted"
