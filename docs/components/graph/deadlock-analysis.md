# Component — Deadlock-candidate analysis

## Responsibility

`analysis::concurrency::deadlock` transforms typed channel, lock, def-use, and
call-target evidence into two immutable cyclic projections:

- process wait-for SCCs (`CommunicationCycle`); and
- conflicting resource lock-order SCCs (`LockOrderCycle`).

It owns no parser, scheduler, or graph mutation.

## Public surface

| API/type | Contract |
|---|---|
| `communication_deadlocks(&CodePropertyGraph)` | ordered advisory findings from both projections |
| `DeadlockFindingKind` | stable communication/lock category with `as_str()` |
| `DeadlockFinding` | sorted process/resource ids, precise witnesses, explanatory detail |
| `DeadlockWitness::Communication` | waiter/producer/channel plus receive/send sites and ranges |
| `DeadlockWitness::LockOrder` | processes, resources, modes, sites/ranges, optional call site |
| `LockKind` / `LockMode` | frontend/adapter-owned typed synchronization metadata |

## Internal flow

![Wait-for construction](../../diagrams/wait-for-deadlock.svg)

![Lock-order construction](../../diagrams/lock-order-deadlock.svg)

Both flows end in `GraphProjection::External` and `projection_sccs`. The module
does not duplicate Tarjan SCC logic. Internal ordered maps select stable
representative producer/acquisition sites; findings retain every internal SCC
edge after deterministic sorting.

## Invariants

1. Every process, resource, and site id exists in the input CPG.
2. Every range equals its witness site's stored `SourceRange`.
3. Unresolved resource identity never becomes a name-derived id.
4. Results and the CPG are unchanged by input node and edge insertion order.
5. The analysis never mutates the source CPG.
6. Communication edges require all known producers to be initially blocked.
7. All-read lock SCCs are not returned as conflicting candidates.
8. Recursive acquisition summaries terminate on the finite resource universe.

## Dependencies

| Dependency | Purpose |
|---|---|
| channel event extraction | ordered `MsgKind` and `ChannelId` evidence |
| function scope helper | top-level process-local skeletons |
| DFG/reference overlay | stable channel and lock identity |
| call targets | interprocedural acquisition summaries |
| `GraphProjection` | canonical CSR wait/order graph |
| SCC decomposition | deterministic cyclic-component recognition |

## Failure and uncertainty behavior

Missing message or lock metadata removes the site from the corresponding
skeleton. An unresolved receive cannot create a wait edge. An unresolved acquire
cannot create an order edge. An unresolved release clears held state. A dangling
call target contributes no summary. These choices prefer visible incompleteness
and false negatives over invented semantic identity.

## Tests

Unit tests cover semantic predicates, direct/interprocedural cycles, breakers,
release/read-mode behavior, recursion, witness validity, immutability, serde,
and generated determinism/rings. Public integration tests consume the crate-root
API and a real Rholang Mode-B tree. DFG regressions protect outer lexical capture
and sibling-function isolation.
