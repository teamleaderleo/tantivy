# Tantivy `prepare_commit` generation-fence probe

## Scope

Fieldwork issue: `teamleaderleo/fieldwork#180`  
Fieldwork report: `teamleaderleo/fieldwork#182`  
Pinned fork base: `667132fa7ab4a30e0c1870d791f23902ebfc6152`  
Exact executed probe head: `b92909ef3d5ac5695d1c85b1b0cb52a03ee51e49`  
Upstream contact authorized: `false`

This is a fork-only characterization. It confirms a mixed worker-generation lifecycle after failed preparation. It does not claim index corruption and does not yet select a production repair.

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

## Executed result

Focused run `30513367302` completed successfully on exact head `b92909ef3d5ac5695d1c85b1b0cb52a03ee51e49`.

Confirmed path:

```text
prepare_commit returns the synthetic old-worker error
replacement generation remains alive
new document admission succeeds
unjoined old worker publishes through the real segment updater
later commit succeeds
old-generation and new-generation terms are both searchable
```

The focused characterization passed `1/1`. The ordinary pull-request Unit tests run `30513367326` also passed its check, `test-none`, `test-all`, and `test-quickwit` jobs.

The result confirms a lifecycle and ownership defect. It does not establish corrupt index contents: the controlled later commit consistently retained both documents.

## Validation correction

The first green focused run used `cargo test --lib test_rollback`. That filter collected zero tests while returning success. The main characterization result is unaffected, but the advertised rollback control was not coverage.

The workflow now:

- lists the available library tests;
- requires each exact named control to exist;
- runs each with `--exact`;
- fails before execution if a named test is absent.

Exact controls:

```text
indexer::index_writer::tests::test_prepare_with_commit_message
indexer::index_writer::tests::test_prepare_but_rollback
indexer::index_writer::tests::test_delete_all_documents_rollback_correct_stamp
indexer::index_writer::tests::test_delete_all_documents_and_rollback
```

This correction requires a new exact-head run before claiming the upgraded adjacent-control receipt.

## Files

- `src/indexer/index_writer/prepare_commit_generation_fence.rs` — deterministic nested unit test;
- `.fieldwork/prepare-commit-generation-fence.patch` — one-line test-module registration;
- `.github/workflows/fieldwork-prepare-commit-generation-fence.yml` — read-only execution carrier.

The patch is intentionally limited to `#[cfg(test)]` registration. No production source behavior changes.

## Execution gate

The repository declares Rust `1.86`, but the generated current dependency graph did not resolve under that declared floor. The successful characterization used Rust `1.88.0`; the separate declared-MSRV/dependency issue is tracked in Fieldwork #200.

The current workflow runs:

```text
rustfmt +1.88.0 --edition 2021 --check src/indexer/index_writer/prepare_commit_generation_fence.rs
cargo +1.88.0 generate-lockfile
cargo +1.88.0 metadata --locked --format-version 1
cargo +1.88.0 test --lib prepare_commit_failure_leaves_next_generation_live_and_accepts_late_old_segment --locked --no-default-features -- --nocapture
four exact adjacent prepare/rollback controls with existence preflight
```

The repository's ordinary pull-request workflows remain separate integration evidence.

## Self-review

An initial test revision used a tokenized `TEXT` field with hyphenated values, which would have made the final exact-term assertions false for an unrelated tokenizer reason. The fixture uses `STRING` so the searchable terms are exact.

A later self-review found the zero-test rollback filter described above. A green command is not evidence when its intended assertion did not run.

A local clone/test attempt could not run because the available container could not resolve GitHub and lacked a Rust toolchain. Those were environment limitations, not target results.

## Repair invariant retained

A complete repair must not return from failed `prepare_commit()` while either of these is true:

- an old worker remains able to publish into the shared updater;
- a replacement generation remains available for new admission.

Admission blocking alone is insufficient. The next candidate should join all old workers before publishing replacements and must retire or rebuild the writer on any worker-join, worker-result, or replacement-spawn failure. The initiating worker error should remain primary if cleanup also fails.

## Evidence class

- source mechanism: `source-read`;
- deterministic Tantivy regression: `target-executed` at `b92909ef3d5ac5695d1c85b1b0cb52a03ee51e49`;
- ordinary repository pull-request gate: `integration-executed` at the same head;
- exact corrected adjacent-control gate: pending current-head rerun;
- production repair: absent.

## Limits

The test uses synthetic ordered join handles to force the error schedule. It exercises real document indexing, segment-updater publication, commit, and search behavior, but it does not inject a real filesystem or `SegmentWriter` failure. A production candidate still needs cleanup-failure and replacement-spawn-failure controls.

No upstream issue, pull request, comment, reaction, branch, or message was created or changed.
