# Stage 6 — Model evolution ownership and recovery decision

**Status: proposed decision; [Issue #41](https://github.com/aschenrot/riesel/issues/41) remains open.** This document extends the [initial migration research](sqlite-model-evolution-research.md). The tested experiments in this PR are not an implementation of a safe migration mechanism.

## Verified results (Linux, pinned Rust 1.100)

**CI: [run 44660463321](https://github.com/aschenrot/riesel/actions/runs/44660463321), PR head `2206ea1d`.** Normal workspace validation and smoke passed; **14/14 model-evolution research fixtures and 2/2 ownership fixtures passed**, with no production provider changes.

| Experiment | Observed result | Not established |
| --- | --- | --- |
| Open v1 `SqliteStore` in one Rust process, commit a test-only migration in another, then let v1 write | An old process **inserted a v1 record after v2 metadata committed**. The v2 provider opened but could not decode the inserted record | Safe online migration, or transaction-only lifecycle exclusion |
| Trial transaction-local source-model check | Rejected the old model while leaving records and metadata unchanged | Did not by itself implement the subsequent [write-fencing fix #45](https://github.com/aschenrot/riesel/pull/45), or fence old binaries retroactively |
| Independent `VACUUM INTO` snapshot and restore | Restored the old schema, UUID-linked note, task identifiers and revision from a separate completed snapshot while source remained v2 | Crash-safe replacement, power-loss immunity or automatic rollback |
| Cooperating process `File::try_lock` | A second process could not acquire the lock while the first held it; it acquired after release | Uncooperative writers or old versions obey the lock, or Windows behavior has been verified |
| Unix rename-and-recreate of an occupied lockfile | **Both original and replacement objects could be locked simultaneously** after pathname replacement | Canonicalizing a directory or a replaceable sibling lockfile guarantees stable owner identity |

This is evidence of a **reproduced stale-provider integrity failure**, not merely an inferred risk. It is also evidence that a bare pathname-based sidecar lock can be bypassed by replacement on Unix. The first safe implementation needs an explicit, enforceable owner-identity contract or an additional write fence; a single successful migration transaction is insufficient.

## Why transaction atomicity is insufficient

**Historical baseline (research revision `06c3116d`):** the linked SQLite provider cached `models: BTreeMap<String, ModelDefinition>` at startup, and its `create` and `update` operations did not re-check persisted metadata within the write transaction. [Corrective PR #45](https://github.com/aschenrot/riesel/pull/45) adds strict write-time checks for the entire model registration and physical format in participating new providers. This is not a migration engine or lifetime ownership lock and does not protect old binaries.

A standalone migration transaction can update JSON records and persisted model definitions consistently yet leave an **older provider already open**. After the transaction releases SQLite's writer lock, that provider can submit a new write using the original model. This can create incompatible records in an otherwise successfully migrated database, invalidating future decoding.

**Decision implication:** an explicit maintenance instruction is not enough unless the implementation can *enforce* an ownership/fencing policy for all cooperating provider processes. Running old binaries after a migration must also be considered.

Evidence: [production provider](../../../packages/storage/sqlite/src/records.rs), [format-v1 open checks](../../../packages/storage/sqlite/src/database.rs), and [new test-only fault probes](../../../packages/storage/sqlite/tests/migration_research.rs). See the final PR evidence for the exact result; the test captures actual effects, not a claim about every OS.

## Ownership alternatives

| Approach | Guarantees within a stated boundary | Limits / failure cases | Relative fit |
| --- | --- | --- | --- |
| Operator-only offline instruction | No implementation changes; migration is performed only after the operator closes apps | No enforcement, forgotten or restarted old process writes undetected | **Insufficient as the only protection** |
| Exclusive **cooperative lifetime lock** using a stable sibling lockfile | Co-operating contender was blocked in the Linux test while another process held the lock | An old version/raw SQL ignores it, and Unix rename/recreate **created a new lock identity**; path aliasing/permissions/platform guarantees require a policy | Useful only with stable protected identity and a second guard; **not sufficient on its own** |
| In-transaction source-model/epoch check on **every** write | Protects participating providers against post-open metadata drift when the check and write share a SQLite transaction; [corrective PR #45](https://github.com/aschenrot/riesel/pull/45) adds this for create/update/delete | Old binaries do **not** implement it; full-model scans incur per-write overhead; reads are not fenced and offline upgrade ownership is still unproven | Necessary write-integrity defense, **not** a substitute for a lifetime ownership/maintenance policy |
| SQLite `BEGIN IMMEDIATE` / `locking_mode=EXCLUSIVE` | Serializes writers during an active SQL transaction; SQLite exclusive mode may constrain connection sharing | Neither a short write transaction nor an implicit PRAGMA provides portable cross-version owner identity and lifecycle fencing; WAL and busy handling need explicit tests | Transaction atomicity is necessary but **not sufficient** as sole lifetime policy |
| External process coordinator / distributed lease | Central ownership and revocation can be composed into deployments | Adds runtime service and failure/lease ambiguity, unnecessary for accepted single-host linked scope | Out of scope |
| Copy/shadow database with atomic swap | Source can remain available until swap, and backup retained | Old providers can keep writing old inode, or diverge after switching the path; WAL and directory/fsync semantics are not established | Not a substitute for owner fencing |

Rust 1.100 has stable [`std::fs::File::lock` / `try_lock`](https://doc.rust-lang.org/std/fs/struct.File.html); these can be evaluated without an additional crate. Their semantics depend on the platform and other participants' cooperation. SQLite itself documents concurrent WAL readers, serialized writers and transaction isolation: [WAL](https://www.sqlite.org/wal.html), [isolation](https://www.sqlite.org/isolation.html), [transaction semantics](https://www.sqlite.org/lang_transaction.html). Neither feature constitutes authorization against malicious/local uncooperative database writers.

### Rollout and compatibility constraint

Any cooperative lock **cannot retroactively stop a pre-upgrade executable**. A safe first migration must require an explicitly supported rollout boundary: stop and verify termination of all older providers, deploy providers that implement the lock/fence, verify ownership, then authorize migration. A fail-closed startup version check alone cannot stop an old provider that was opened before the schema switch.

Lock-file naming and path resolution require explicit design: canonicalize the parent directory, prevent changing the lock identity by deleting/renaming a lock file while held, reject unsafe aliases, specify permissions, and ensure no database creation or access in *inspection/preflight*. Lock acquisition happens at provider startup, not registration. An unsupported owner scenario must fail before mutation.

## Provisional implementation contract, for accountable Gate B decision

1. **Normal startup stays strict:** no implicit migrations or schema rewriting, no fallback to ephemeral storage, and no database access in `App::preflight`.
2. **Maintenance-only authorization:** a distinct explicit command with a source/target Rust schema identity, mutation preview and a separate operator-approved execute operation. Preview must not modify source data; startup and execute must not conflate user intent.
3. **Enforce owner lifetime:** require a cooperating whole-lifetime lock held by normal linked providers, and an exclusive maintenance lock held through backup verification, transactional migration, output validation and finalization. Reject if ownership cannot be proven. Verify behavior after process crash and independent processes. Document that uncooperative old binaries/direct SQLite writers are outside the enforceable scope.
4. **Guard against stale writes:** evaluate in-transaction schema/epoch checking in new providers before accepting any approach that allows independently running old code during or after upgrade; otherwise **require old code to be stopped and prevented from restarting**. The transaction test alone cannot claim this property.
5. **Require verified pre-mutation backup:** produce a consistent independent backup (SQLite backup API or `VACUUM INTO`), validate its `integrity_check`, format v1 and model metadata, confirm that it can reopen with the source model set, and retain it. Do not copy a live WAL `.db` alone. Restore is a separately controlled offline operation, **not** silent rollback of post-migration writes.
6. **Atomic, bounded transformation:** compare all expected source model definitions, independently derive target models, validate old/new records and cross-model invariants, preserve UUIDs, revise affected record revisions atomically, and reject overflow, unsupported schema changes and interrupted writes. Clearly bound data size, transaction duration, disk/WAL headroom and error diagnostics; defer larger/online migrations.
7. **Compatibility and downgrade:** retain physical format `user_version=1` when only changing JSON shapes and existing model metadata. Introduce a new physical version only for an actual layout/metadata encoding change. Old binaries must fail closed on incompatible new models; downgrading requires a verified pre-upgrade snapshot and deliberate loss-of-newer-write disclosure.
8. **Operational scope:** linked provider, single local host and supported local filesystem only. No public API, remote/managed parity, filesystem-level power-loss proof, distributed lease or generic SQL migration framework.

**No architecture is accepted by this proposal.** The final Gate B decision must explicitly accept/revise/reject the policy and name the accountable maintainer before opening implementation issues.

## Required decision-critical verification

- [ ] Reproduce the stale provider write through **a separate running Rust process** using the current shipped store, after the test-only migration commits; demonstrate the new store fails to decode the resulting record.
- [ ] Demonstrate a transaction-local schema check rejecting an old definition and preserving the database, and identify the migration needed to roll out this check.
- [ ] Verify the standalone backup/recovery fixture preserves the old schema, UUIDs and revisions; distinguish this controlled copy from crash-safe in-place rollback.
- [ ] Prove a cooperating lifetime-lock prototype on the pinned toolchain across processes: second owner cannot acquire while first lives; only after the first exits; a maintenance action must refuse active owners. Test aliases, lockfile replacement and startup errors before approving a concrete lock algorithm.
- [ ] Test and document stable source/target schema identification and a minimal typed historical-model API without duplicating Rust schemas.
- [ ] Separate hard operational guarantees (power-loss behavior, disk-full, permissions, long-running migration resource budget, backups) from those actually tested. Test on Linux; Windows remains a manual pre-release gate.
- [ ] Record an explicit Gate B **accept/revise/defer** verdict and any remaining exclusions in Issue #41. Only then create independently scoped implementation issues.

## Review outcome required

The stale-provider failure **is now reproduced**, and Unix lockfile replacement also bypasses naive pathname identity. Do not ship migration until an enforceable owner-fencing contract, a write-integrity guard and a legacy-process rollout are accepted and tested. Reject unsupported “safe migration” claims rather than relying on shutdown instructions alone.

If the first safe scope cannot establish lock identity, maintenance exclusivity, backup verification and bounded failure behavior at reasonable cost, defer migration implementation and retain current exact-schema startup rejection.
