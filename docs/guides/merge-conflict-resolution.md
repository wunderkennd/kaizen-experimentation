# Merge Conflict Resolution

Strategies for the specific file types that commonly conflict in the Kaizen multi-agent workflow.

## Generated / Build Artifact Files

These should not be in git. If you encounter a conflict in one, remove it from tracking permanently:

```bash
git rm --cached <file>
echo "<pattern>" >> .gitignore
git rebase --continue
```

Common offenders: `tsconfig.tsbuildinfo`, `.Jules/palette.md`, `node_modules/`, `.next/`, `target/`.

## Multi-Branch Rebase After File Removal

When a file is removed from tracking on `main` and multiple open branches still have it:

```bash
git checkout main && git pull origin main

for branch in $(gh pr list --json headRefName --jq '.[].headRefName'); do
  echo "=== Rebasing $branch ==="
  git checkout "$branch"
  git rebase main || {
    git rm --cached ui/tsconfig.tsbuildinfo 2>/dev/null
    git rm --cached .Jules/palette.md 2>/dev/null
    git rm ui/tsconfig.tsbuildinfo 2>/dev/null
    git rm .Jules/palette.md 2>/dev/null
    git rebase --continue
  }
  git push --force-with-lease origin "$branch"
done
git checkout main
```

Preview which branches will conflict first:
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

Proto conflicts require manual resolution — they affect all downstream codegen.

If two branches both added new fields to the same message:
1. Check that tag numbers don't collide
2. Check that field names don't conflict
3. Accept both additions
4. Run `buf lint proto/` and `buf breaking proto/ --against .git#branch=main`

If two branches changed the same field:
1. Resolve in favor of the branch that followed the ADR protocol
2. File a follow-up issue for the other branch to adapt

## Cargo.lock Conflicts

```bash
git checkout --theirs Cargo.lock
cargo generate-lockfile
git add Cargo.lock
git rebase --continue
```

The lockfile is deterministic from `Cargo.toml` — it doesn't matter which version you start with.

## go.sum / go.work.sum Conflicts

```bash
git checkout --theirs go.sum go.work.sum
go mod tidy
git add go.sum go.work.sum
git rebase --continue
```

Both files are deterministic from `go.mod` / `go.work`.

## Summary

| File Type | Strategy | Automate? |
| --- | --- | --- |
| Build artifacts | Remove from git entirely | Yes (`.gitignore` + `git rm --cached`) |
| `Cargo.lock` | Accept either, `cargo generate-lockfile` | No (manual but quick) |
| `go.sum` / `go.work.sum` | Accept either, `go mod tidy` | No (manual but quick) |
| Proto schema | Never auto-resolve, check tag numbers | No (requires judgment) |
| Source code | Standard merge resolution | No (requires judgment) |
