# #193 — Real OS process conditional revision/recovery proof (research only)

**Status:** Test-only investigation for accepted [V2 source/revision invariants](https://github.com/aschenrot/riesel/issues/111#issuecomment-7075163935), now tracked as the production implementation outcome [#193](https://github.com/aschenrot/riesel/issues/193). No production plan store, app identity, authorization, public document codec, C3 activation or runtime API is implemented here.

## Decision-critical question

Can distinct actual OS processes commit competing semantic/layout revisions against a shared application-owned **local transaction boundary**, while maintaining immutable accepted plan history, detecting stale source epochs and recovering from writer death before/after transaction commit? A proof is needed before choosing a storage adapter, rather than inferring multi-process guarantees from the [#185 in-process/reopened-connection research](https://github.com/aschenrot/riesel/pull/185).

## Method and reproducibility

Research fixture: `packages/storage/sqlite/tests/v2_multi_process_authority_research.rs`. Uses real `riesel_app::ExecutionPlanDraft::validate` and its **Serialize-only inspection projection**, SQLite WAL/FULL with immediate transactions, and **fresh OS child processes** spawned from the test executable with explicit private per-test environment variables. The parent observes private ready/go files for a controlled simultaneous race; the child has a bounded wait and is killed explicitly at a pre-commit or post-commit boundary.

```sh
cargo test --locked -p riesel_storage_sqlite --test v2_multi_process_authority_research
cargo validate --check
cargo smoke
```

The parent test opens a fresh temporary DB and does **not** use public `RecordStore`: that contract provides only one-record CAS, not an atomic V2 aggregate with distinct semantic/layout state.

## Cases to verify with exact CI

1. Two actual child OS processes both attempt the same semantic predecessor/owner generation. One `UPDATE ... WHERE expected` wins; only its immutable revision row appears, active revision unchanged.
2. Two child OS processes race the **layout** predecessor; only one wins, semantic revision and active graph remain untouched.
3. Kill writer after updating semantic revision **inside an uncommitted transaction**; reopen and prove rollback, then accept a valid new writer.
4. Commit an active-revision pointer and receipt inside one local transaction; kill the process **after commit, before simulated reply**, reopen and prove both values are present.
5. Transfer source generation from Rust to document with a conditional writer; demonstrate old semantic and layout writers fail while newly fenced writers succeed.

**Do not overclaim:** This proves local transactional behavior under OS-process death only. It does not prove filesystem/power failure durability, secure source principal or installed-artifact attestation, single semantic ownership across hosts, application runner activation, authenticated cross-process editor traffic, idempotent nontransactional effects, or lossless document migration. A persisted active pointer alone is not proof that the real session-owned runner executed/activated a plan. Treat the test ledger as an alternative-comparison fixture, not production architecture or an approved universal SQLite dependency.

## Alternatives and next gate

- `RecordStore` single-record aggregate: simple reuse but unwanted semantic/layout contention, no multi-object atomicity or snapshot listing; needs quantified design evidence.
- Application-owned transaction store (SQLite local reference): plausible semantic/layout/source conditional update owner with independently versioned backend interface; costs schema migration, reliable installed-artifact provenance, lock contention and recovery policy.
- File rename + lock/fsync or append-only log: possible but multi-writer fencing, crash durability, replay/compaction and lifecycle must be measured before selecting.
- Mandatory Manager store: incompatible with accepted standalone headless app and independent app-owned execution authority.

The **production #193 work** must still choose a justified, portable abstraction, verify real owner identity, handle layout structural rebases, snapshot/writer contention, source-transfer compatibility reports and backup/migration under actual app publisher and session identity. This standalone research PR should remain draft/unmerged until evidence is verified and disposition recorded; it does not close #193.
