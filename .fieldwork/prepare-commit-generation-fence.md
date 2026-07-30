# Tantivy `prepare_commit` fail-closed repair trial

## Scope

Fieldwork issue: `teamleaderleo/fieldwork#180`  
Fieldwork report: `teamleaderleo/fieldwork#182`  
Evidence-audit note: `teamleaderleo/fieldwork#225`  
Pinned fork base: `667132fa7ab4a30e0c1870d791f23902ebfc6152`  
Characterization head: `b92909ef3d5ac5695d1c85b1b0cb52a03ee51e49`  
Corrected control head: `ed3d4b4b82b34e0f214705ef55e6e8eaa84e60cd`  
Upstream contact authorized: `false`

This is a fork-only production-repair trial. It is not an upstream proposal and is not accepted until the exact target workflow executes.

## Confirmed defect

The repository-native characterization proved that current `prepare_commit()` can:

1. start a replacement worker after one old worker settles;
2. return a later old-worker error;
3. leave the replacement generation alive for new admission;
4. abandon an unjoined old worker;
5. permit that old worker to publish a real segment later;
6. commit both the late old segment and post-error new work.

The controlled index remained queryable. The confirmed defect is lifecycle and generation ownership, not demonstrated index corruption.

## Repair invariant

`prepare_commit()` must not publish the next worker generation until every old worker has settled successfully.

If any old worker join or worker result fails:

- continue joining every remaining old handle;
- preserve the first failure as the returned error;
- rollback/rebuild before returning;
- reject late old-generation publication from the durable post-error state.

If replacement startup fails after old workers settle:

- rollback/rebuild the partial replacement generation;
- return the replacement-spawn error.

If rollback/rebuild itself fails:

- keep the initiating error primary;
- log cleanup failure as secondary evidence;
- kill the updater and current writer status;
- drop the sender so remaining state fails closed rather than accepting unowned work.

## Candidate shape

The patch adds one private recovery helper and changes `prepare_commit()` ordering:

```text
recreate channel
join every old handle, collecting first failure
if old failure: rollback/rebuild, then return primary error
start the full replacement generation
if spawn failure: rollback/rebuild, then return spawn error
prepare commit only after all of the above succeeds
```

No generation token or second lifecycle owner is introduced. The existing rollback/rebuild path remains the authority for returning to committed state.

## Repair regression

The deterministic private-access test retains the three ordered synthetic handles and the real indexing path:

- first old worker succeeds;
- second old worker returns the synthetic primary error;
- third old worker owns a real document and publishes through the real `SegmentUpdater`;
- a helper releases the final worker after the failure worker starts;
- `prepare_commit()` must wait for all handles;
- the old worker's `index_documents()` call must settle before `prepare_commit()` returns;
- the returned error must still be the synthetic second-worker error;
- the writer must contain a complete rebuilt worker generation;
- a new document must be accepted and committed;
- the old-generation term must remain absent;
- the new-generation term must be searchable.

This directly distinguishes “old work reached the previous updater” from “old work survived rollback into a later searchable commit.”

## Execution gate

The workflow:

1. checks out the exact PR head rather than a merge ref;
2. applies the exact repair patch and test registration;
3. format-checks the repaired source and test;
4. generates one locked dependency graph with Rust 1.88;
5. records dependency and patch digests;
6. preflights the exact repair test name;
7. runs it with `--exact`;
8. preflights and runs four exact adjacent prepare/rollback controls;
9. leaves the ordinary repository Unit Tests workflow as separate integration evidence.

## Self-review limits

The candidate still lacks a target-native injected replacement-thread spawn failure. `thread::Builder::spawn()` failure is difficult to force without a narrow test seam.

The candidate also does not yet execute a rollback-construction failure. The fallback kill path is source-reviewed but not target-executed.

These are real remaining gates, not reasons to discard the primary repair test. Promotion beyond a fork trial requires:

- one bounded spawn-failure injection seam or equivalent deterministic control;
- one cleanup-failure control proving no admission remains possible;
- exact primary-versus-secondary error assertions;
- complete repository gate on the exact candidate head.

## Evidence class

- original defect: `target-executed`;
- corrected characterization controls: `target-executed`;
- repair source: `target-test-prepared` until the new workflow completes;
- spawn-failure cleanup: `source-read` only;
- rollback-failure cleanup: `source-read` only;
- public upstream interaction: absent.

No upstream issue, pull request, comment, reaction, branch, email, or message was created or changed.
