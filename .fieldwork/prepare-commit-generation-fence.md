# Tantivy `prepare_commit` generation-fence probe

## Scope

Fieldwork issue: `teamleaderleo/fieldwork#180`  
Fieldwork report: `teamleaderleo/fieldwork#182`  
Pinned fork base: `667132fa7ab4a30e0c1870d791f23902ebfc6152`  
Upstream contact authorized: `false`

This is a fork-only characterization. It does not propose a production repair and does not claim index corruption before target execution.

## Source-order question

`IndexWriter::prepare_commit()` currently:

1. replaces the document channel and `IndexWriterStatus`;
2. moves out the prior worker join handles;
3. joins one old worker;
4. immediately starts one replacement worker on the new channel;
5. repeats until a join, worker, or replacement-spawn error returns through `?`.

A later error can therefore occur after partial replacement startup. Remaining old join handles are dropped rather than joined. Their threads are not cancelled by dropping the handles, and their worker bombs refer to the old status generation rather than the newly installed status.

## Deterministic target test

The nested test module has private access without adding production API surface. It:

1. cleanly retires the repository-created workers;
2. installs three ordered synthetic join handles:
   - one successful old worker;
   - one failing old worker;
   - one blocked old worker that owns a real document and the real `SegmentUpdater`;
3. calls `prepare_commit()`;
4. requires the second worker error;
5. verifies that the first successful join already created a replacement worker and the new writer status remains alive;
6. admits a new-generation document after the failed preparation;
7. releases the third, now-detached old worker;
8. runs Tantivy's real `index_documents()` path and waits for segment-updater publication;
9. commits again;
10. requires both the late old-generation document and the newly admitted document to be searchable.

This separates three claims:

- a replacement generation starts before all old workers settle;
- an unvisited old worker continues after its `JoinHandle` is dropped;
- a later commit can include both post-error new work and late prior-generation publication.

## Files

- `src/indexer/index_writer/prepare_commit_generation_fence.rs` — deterministic nested unit test;
- `.fieldwork/prepare-commit-generation-fence.patch` — one-line test-module registration;
- `.github/workflows/fieldwork-prepare-commit-generation-fence.yml` — read-only execution carrier.

The patch is intentionally limited to `#[cfg(test)]` registration. No production source behavior changes.

## Execution gate

The workflow uses Rust `1.86.0`, matching the package's declared minimum, and runs:

```text
cargo +1.86.0 fmt --all -- --check
cargo +1.86.0 test --lib prepare_commit_failure_leaves_next_generation_live_and_accepts_late_old_segment --locked --no-default-features -- --nocapture
cargo +1.86.0 test --lib test_prepare_ --locked --no-default-features
cargo +1.86.0 test --lib test_rollback --locked --no-default-features
```

The repository's ordinary pull-request workflows remain separate integration evidence.

## Self-review

An initial test revision used a tokenized `TEXT` field with hyphenated values, which would have made the final exact-term assertions false for an unrelated tokenizer reason. The fixture now uses `STRING` so the searchable terms are exact.

A local clone/test attempt could not run because the available container could not resolve GitHub. That is an environment limitation, not a target result. No local execution is claimed.

## Evidence class

- source mechanism: `source-read`;
- deterministic Tantivy regression: `target-test-prepared`;
- target execution: absent until Actions completes;
- production repair: absent;
- full repository gate: absent.

## Stop conditions

Stop or rewrite the hypothesis if execution shows that:

- the replacement worker is not live after the returned error;
- the detached old worker cannot publish through the shared updater;
- the later commit rejects or excludes either generation;
- or the harness fails before reaching the intended worker-order assertions.

No upstream issue, pull request, comment, reaction, branch, or message was created or changed.
