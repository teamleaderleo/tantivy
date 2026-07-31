# Tantivy `prepare_commit` fail-closed repair trial

State: `target-test-prepared`

## Scope

Fieldwork issue: `teamleaderleo/fieldwork#180`  
Fieldwork report: `teamleaderleo/fieldwork#182`  
Evidence-audit note: `teamleaderleo/fieldwork#225`  
Pinned fork base: `667132fa7ab4a30e0c1870d791f23902ebfc6152`  
Characterization head: `b92909ef3d5ac5695d1c85b1b0cb52a03ee51e49`  
Corrected control head: `ed3d4b4b82b34e0f214705ef55e6e8eaa84e60cd`  
Candidate source/test head: `05b2c56a597794fb2cece6364c04819b27b0acf2`  
Upstream contact authorized: `false`

This is a fork-only production-repair trial. Acceptance requires exact target execution and ordinary repository gates.

## Confirmed defect

The repository-native characterization proved that current `prepare_commit()` can:

1. start a replacement worker after one old worker settles;
2. return a later old-worker error;
3. leave the replacement generation alive for new admission;
4. abandon an unjoined old worker;
5. permit that old worker to publish a real segment later;
6. commit both the late old segment and post-error new work.

The controlled index remained queryable. The confirmed defect is lifecycle and generation ownership; index corruption was outside that execution.

## Repair invariant

`prepare_commit()` must publish the next worker generation only after every old worker settles successfully.

If any old worker join or worker result fails:

- continue joining every remaining old handle;
- preserve the first failure as the returned error;
- rollback and rebuild before returning;
- exclude late old-generation publication from the durable post-error state.

If replacement startup fails after old workers settle:

- rollback and rebuild the partial replacement generation;
- preserve the replacement-spawn error;
- return with a complete usable worker generation.

If rollback or rebuild fails:

- keep the initiating error primary;
- log cleanup failure as secondary evidence;
- kill updater and writer status authority;
- drop the sender so later admission fails closed.

## Candidate design

The patch adds one private recovery helper and changes `prepare_commit()` ordering:

```text
recreate channel
join every old handle, collecting first failure
if old failure: rollback/rebuild, then return primary error
start the full replacement generation
if spawn failure: rollback/rebuild, then return spawn error
prepare commit only after all of the above succeeds
```

No generation token or second lifecycle owner is introduced. Existing rollback/rebuild remains the authority for returning to committed state.

Two test-only fields provide deterministic failure injection:

- `fail_worker_spawn_after` fails replacement creation after a configured number of successful starts;
- `fail_next_recovery_rollback` fails the recovery call before writer reconstruction.

They compile only under `cfg(test)` and leave the production API unchanged.

## Native regressions

### Late old-worker settlement

The first regression retains three ordered synthetic handles and the real indexing path:

- one old worker succeeds;
- one returns the primary error;
- one owns a real document and publishes through the real `SegmentUpdater` after the failure starts;
- `prepare_commit()` waits for every handle;
- rollback rebuilds a complete generation;
- later new work commits successfully;
- the old-generation term remains absent;
- the new-generation term is searchable.

This distinguishes “old work reached the previous updater” from “old work survived rollback into a later searchable commit.”

### Partial replacement generation

The second regression retires the original workers, starts one replacement worker, then injects failure on the next replacement spawn. It requires:

- the spawn error remains authoritative;
- the partial generation is rolled back;
- a complete fresh generation exists before return;
- the writer remains alive;
- later work commits and becomes searchable.

### Recovery rollback failure

The third regression injects an old-worker primary error and then fails recovery rollback. It requires:

- the old-worker error remains the returned error;
- the cleanup error never replaces it;
- writer status is dead;
- later document admission is rejected.

## Execution gate

The workflow:

1. checks out the exact PR head;
2. applies the exact candidate with `git apply --check`;
3. checks the complete patched repository diff;
4. format-checks all patched Rust source;
5. generates one locked dependency graph with Rust 1.88;
6. records dependency and patch digests;
7. preflights and executes all three exact failure regressions;
8. preflights and executes four exact adjacent prepare/rollback controls;
9. leaves ordinary repository Unit Tests as separate integration evidence.

## Evidence boundary

- original defect: `target-executed`;
- corrected characterization controls: `target-executed`;
- old-worker repair regression on predecessor head: `target-executed`;
- expanded source and three-test candidate: `target-test-prepared` until exact execution settles;
- replacement-spawn recovery: deterministic target test prepared;
- rollback-failure fail-closed authority: deterministic target test prepared;
- complete ordinary unit gate on expanded candidate: pending;
- performance impact: unmeasured;
- public upstream interaction: absent.

## Current disposition

**HOLD production acceptance until the expanded exact workflow and ordinary Unit Tests pass on the same candidate head.**

No public upstream issue, pull request, comment, reaction, branch, email, or message was created or changed.
