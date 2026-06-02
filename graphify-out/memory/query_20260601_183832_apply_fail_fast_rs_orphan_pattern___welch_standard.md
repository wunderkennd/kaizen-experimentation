---
type: "query"
date: "2026-06-01T18:38:32.068199+00:00"
question: "Apply fail_fast.rs orphan pattern + welch_standard_error misfile pattern systematically across the workspace — find every analogous case."
contributor: "graphify"
source_nodes: ["fail_fast.rs", "custom_migrator.rs", "extract_select", "unwrap_nested", "strip_table_alias", "collect_and_predicates", "tier1.rs", "tier2.rs", "migration", "translate_filtered_aggregation"]
---

# Q: Apply fail_fast.rs orphan pattern + welch_standard_error misfile pattern systematically across the workspace — find every analogous case.

## Answer

Two patterns applied. (1) ORPHAN-FILE: scanned all 101 production .rs files for missing 'mod {stem};' declarations. Only 2 candidates: custom_migrator.rs (false positive — valid src/bin/ binary target per Cargo.toml [[bin]]) and fail_fast.rs (true orphan, already filed #584). Conclusion: fail_fast.rs is the ONLY file-level orphan in the workspace. (2) MISFILE: scanned all nodes whose graph community has a strict (>50%) owner-file different from their own source file. Most candidates were graph artifacts (production .rs files 'pulled toward' their own /tests/_golden.rs because test files have larger node counts). One genuine finding survived: 7 pub(crate) fn SQL-AST helpers in crates/experimentation-management/src/migration/tier1.rs used heavily by tier2.rs — including unwrap_nested (20+18=38 calls), strip_table_alias (11), extract_select (4 of 7 calls from tier2). tier2.rs exposes 0 pub(crate) fn of its own. Filed #587 to extract these into migration/sql_ast.rs sibling module. The dependency direction tier1<-tier2 becomes sql_ast<-tier1, sql_ast<-tier2. Same pattern as welch_standard_error finding but at module level — a junk-drawer file silently exporting primitives that belong in a shared location.

## Source Nodes

- fail_fast.rs
- custom_migrator.rs
- extract_select
- unwrap_nested
- strip_table_alias
- collect_and_predicates
- tier1.rs
- tier2.rs
- migration
- translate_filtered_aggregation