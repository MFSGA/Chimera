# Profile ref-alignment runner

The runner follows the ordered task pack in `docs/profile-local-model-task-pack.md`. It will not plan or apply a later task while an earlier dependency is incomplete. T00 is a read-only call-chain audit; T01 is the first copy-eligible task (its files were already identical to ref, so T04 was the first task that wrote copied code).

The local model only returns a JSON selection among mappings already listed in a task manifest. Ollama constrains that response with a JSON Schema. The runner rejects paths outside the task pack's target allowlist, unknown mapping/replacement IDs, ref baseline drift, modified `ref/`, dirty/untracked target files, symlinks, stale hashes, and uncertain plans. Applying a plan copies exact ref bytes, optionally replacing a declared prefix or a marker-bounded block, or inserting exact marker-bounded ref blocks. A changed target can be used only when its current SHA-256 is declared as the reviewed base. The runner only applies literal replacements already declared in the manifest; it does not format or synthesize copied code.

Copy modes are `whole`, `replace-prefix`, `replace-block`, `insert-before-marker`, and `insert-blocks-before-marker`. `replace-block` copies a bounded ref range between unique source markers over a bounded target range between unique target markers; all bytes outside the target range remain untouched.

## Commands

From the repository root:

```powershell
node scripts/ref-align.mjs status
node scripts/ref-align.mjs prompt --task T00
```

After Codex has checked a task's evidence, record that stage so the next dependency becomes available:

```powershell
node scripts/ref-align.mjs complete --task T00 --evidence "Codex reviewed the read-only T00 report and checked its path/symbol claims against ref."
```

For a copy stage, create a task manifest based on that task's `允许改动` list. Every `target` must be in that list. Keep plans and manifests under `.tmp/ref-align/`; the directory is ignored by Git. Example:

```json
{
  "version": 1,
  "taskId": "T01",
  "refCommit": "232321d52121fe8bb25cb2a090d814129cb50c55",
  "goal": "Migrate ProfileId, path/source types, and module exports by copying the corresponding ref files. Preserve existing validation unless it conflicts with ref.",
  "candidates": [
    {
      "id": "profile-id",
      "source": "backend/nyanpasu-config/src/profile/id.rs",
      "target": "backend/chimera-config/src/profile/id.rs",
      "strategy": "copy",
      "description": "ProfileId newtype and parsing contract",
      "replacements": []
    }
  ]
}
```

Generate and review a model plan, then apply only that exact reviewed plan:

```powershell
node scripts/ref-align.mjs plan --task T01 --manifest .tmp/ref-align/T01.json --out .tmp/ref-align/plans/T01.json
Get-Content .tmp/ref-align/plans/T01.json
node scripts/ref-align.mjs apply --task T01 --manifest .tmp/ref-align/T01.json --plan .tmp/ref-align/plans/T01.json --reviewed-sha256 <sha256-checked-by-Codex>
```

The plan hash is printed by `plan`. Codex should independently check the task mapping and each selected replacement against `ref/` before passing that hash to `apply`. A task target with existing local changes is never overwritten; route such a file to a reviewed merge step. The workflow state is local and ignored; keep the versioned migration record in `docs/ref-alignment-progress.md` current as tasks finish.

The runner uses Node's built-in modules and Ollama's local `/api/chat` endpoint; no extra package or API key is needed. The default model is `qwen2.5-coder:14b`; override it with `--model`. Use `--ollama-url` if Ollama is not listening at `http://localhost:11434`.
