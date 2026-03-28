# Merge Conflict Resolution

This guide covers conflict resolution strategies for the specific file types that commonly conflict in the Kaizen multi-agent workflow.

## Status Files (`docs/coordination/status/`)

**Rule: Always take the incoming version.**

Status files are single-writer (Agent-4 is the only writer of `agent-4-status.md`) and append-only. The newer write is always the more accurate state. There is no meaningful "merge" of two status file versions.

### Manual resolution (during rebase)

```bash
git checkout --theirs docs/coordination/status/*.md
git add docs/coordination/status/*.md
git rebase --continue
```

### Permanent automation (recommended)

Add a custom merge driver to `.gitattributes`:

```
docs/coordination/status/** merge=theirs-status
```

Configure the driver (run once, or add to your `.gitconfig`):

```bash
git config merge.theirs-status.name "Always accept incoming status file"
git config merge.theirs-status.driver "cp %B %A"
```

After this, status file conflicts are resolved automatically on every merge/rebase — no manual intervention ever.

## Generated / Build Artifact Files

These files should not be tracked in git at all. If you encounter a conflict in one, the fix is to remove it from tracking permanently.

### Common offenders

| File | What it is | Fix |
| --- | --- | --- |
| `ui/tsconfig.tsbuildinfo` | TypeScript incremental compilation cache | `git rm --cached`, add to `.gitignore` |
| `.Jules/palette.md` | Jules agent session state | `git rm --cached`, add `.Jules/` to `.gitignore` |
| `node_modules/` | npm packages | Should already be in `.gitignore` |
| `.next/` | Next.js build output | Should already be in `.gitignore` |
| `target/` | Rust build output | Should already be in `.gitignore` |

### Resolution

```bash
# Remove from tracking (keeps local copy)
git rm --cached <file-or-directory>

# Add to .gitignore
echo "<pattern>" >> .gitignore

# Commit the removal
git commit -m "chore: stop tracking <file> (build artifact)"
```

TypeScript regenerates `tsconfig.tsbuildinfo` on the next build. Jules regenerates `palette.md` on the next session. No information is lost.

## Multi-Branch Rebase After File Removal

When you remove a file from tracking on `main` (e.g., `tsconfig.tsbuildinfo`) and multiple open branches still have it, every branch will conflict on rebase. Resolve them all in one pass:

```bash
git checkout main
git pull origin main

for branch in $(gh pr list --json headRefName --jq '.[].headRefName'); do
  echo "=== Rebasing $branch ==="
  git checkout "$branch"
  git rebase main || {
    # Accept the deletion for known build artifacts
    git rm --cached ui/tsconfig.tsbuildinfo 2>/dev/null
    git rm --cached .Jules/palette.md 2>/dev/null
    git rm ui/tsconfig.tsbuildinfo 2>/dev/null
    git rm .Jules/palette.md 2>/dev/null
    git rebase --continue
  }
  git push --force-with-lease origin "$branch"
  echo "=== Done: $branch ==="
done

git checkout main
```

If some branches have additional conflicts beyond the removed files, the rebase will pause for manual resolution. Preview which branches will conflict first:

```bash
for branch in $(gh pr list --json headRefName --jq '.[].headRefName'); do
  echo -n "$branch: "
  git checkout "$branch" 2>/dev/null
  if git rebase --dry-run main 2>&1 | grep -q "CONFLICT"; then
    echo "CONFLICT"
  else
    echo "clean"
  fi
done
git checkout main
```

## Proto Schema Conflicts

Proto conflicts require careful resolution — they affect all downstream codegen. Never auto-resolve proto conflicts.

If two branches both added new fields to the same proto message:
1. Check that tag numbers don't collide
2. Check that field names don't conflict
3. Accept both additions (both tag numbers are valid)
4. Run `buf lint proto/` and `buf breaking proto/ --against .git#branch=main` after resolution

If two branches changed the same field or renamed a message:
1. This is a breaking change that should have gone through the ADR process
2. Resolve in favor of the branch that followed the protocol (has an ADR, was reviewed)
3. File a follow-up issue for the other branch to adapt

## Cargo.lock Conflicts

`Cargo.lock` conflicts are common when two branches add different dependencies. Resolution:

```bash
# Accept either version, then regenerate
git checkout --theirs Cargo.lock   # or --ours
cargo generate-lockfile            # regenerates from Cargo.toml
git add Cargo.lock
git rebase --continue
```

The lockfile is deterministic from `Cargo.toml` — it doesn't matter which version you start with as long as you regenerate it.

## go.sum / go.work.sum Conflicts

Same approach as Cargo.lock:

```bash
git checkout --theirs go.sum go.work.sum
go mod tidy
git add go.sum go.work.sum
git rebase --continue
```

Both files are deterministic from `go.mod` / `go.work` — regeneration is always correct.

## Summary of Strategies

| File Type | Strategy | Automate? |
| --- | --- | --- |
| Status files | Always accept incoming | Yes (`.gitattributes` merge driver) |
| Build artifacts | Remove from git entirely | Yes (`.gitignore` + one-time `git rm --cached`) |
| `Cargo.lock` | Accept either, regenerate | No (manual but quick) |
| `go.sum` / `go.work.sum` | Accept either, `go mod tidy` | No (manual but quick) |
| Proto schema | Never auto-resolve, check tag numbers | No (requires judgment) |
| Source code | Standard merge resolution | No (requires judgment) |
