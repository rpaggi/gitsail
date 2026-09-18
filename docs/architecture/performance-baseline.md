# Performance baseline (EPIC-23: T-226/US-115, T-229/US-118)

SAD §32 lists initial engineering targets in prose ("first history page
should avoid scanning full history", "large diffs and blame results should
support cancellation") and is explicit that **no hard millisecond SLA is
frozen before representative benchmarks exist**. This document is that
first representative measurement, and the budgets at the bottom are derived
from it — not invented ahead of time.

It is not a promise about every machine's performance; it is one
reproducible data point, with the environment recorded, to catch future
regressions against.

## How to reproduce

```
cargo test -p gitsail-git --test performance_baseline --release -- --ignored --nocapture
```

The test (`crates/gitsail-git/tests/performance_baseline.rs`) is `#[ignore]`d
by default — it builds three synthetic fixtures (100 / 1,000 / 5,000
commits, 3 files touched per commit) via `git fast-import`, so it is not part
of the default `cargo test --workspace` run.

## Environment measured

- Date: 2026-09-18
- OS: Linux 6.18.33.2-microsoft-standard-WSL2 (WSL2), x86_64, 12 logical CPUs
- Git: 2.43.0
- Build: `cargo test --release` (release profile; debug-profile timings are
  higher and dominated by the same fixed per-process floor described below)

## Measured results

| scale  | commits | files/commit | first page log | first page graph | full-history diff | blame (hot file) | cancelled diff | RSS before (KB) | RSS after (KB) |
|--------|---------|---------------|-----------------|-------------------|--------------------|-------------------|-----------------|------------------|-----------------|
| small  | 100     | 3             | 20.71 ms        | 15.4 µs           | 20.53 ms           | 20.64 ms          | 21.16 ms        | 2872             | 3272            |
| medium | 1,000   | 3             | 20.70 ms        | 13.4 µs           | 20.59 ms           | 20.48 ms          | 21.04 ms        | 3292             | 3368            |
| large  | 5,000   | 3             | 20.63 ms        | 12.8 µs           | 20.50 ms           | 20.57 ms          | 21.13 ms        | 3380             | 3704            |

"First page log", "full-history diff", "blame (hot file)", and "cancelled
diff" are each dominated by spawning one `git` subprocess (raw command
execution here is far below 1 ms for all three scales) — see the next
section for why they don't grow with repository size and why they are all
so close to 20 ms regardless of operation.

## Finding: a fixed ~20 ms floor per Git invocation, from process-wait polling

Every timed operation above lands in a narrow 20–21 ms band, **independent
of history size** (100 vs. 5,000 commits makes no measurable difference).
This is not a coincidence: `GitProcessRunner::wait_with_timeout`
(`crates/gitsail-git/src/runner.rs`) polls the child process's exit status
in a loop gated by `POLL_INTERVAL = Duration::from_millis(20)`. A `git`
invocation that actually finishes in, say, 2 ms still only gets noticed on
the loop's next 20 ms tick, so short-lived invocations are rounded up to
~one poll interval almost every time.

This matters for T-226/US-115's "incremental and responsive" goal: a
frontend issuing many small, fast Git calls in quick succession (e.g.
blaming several files while scrolling, or paging through history one small
step at a time) pays this ~20 ms tax on *every single call*, even though the
underlying `git` process itself is nowhere near that slow. It is not a
regression introduced by this epic's changes — it predates T-226 — but this
is the first time it has been measured and written down, which is exactly
what this story asked for ("meça e registre, não prometa").

This document only records the finding. Lowering `POLL_INTERVAL`, or
switching to a wait mechanism that does not poll at all (e.g. blocking on
the child directly, off the calling thread), is a real, concrete follow-up
opportunity — out of scope for T-226/US-115's four criteria as written, and
deliberately not attempted as a drive-by change here. A future task can cite
this document as the measured motivation.

## Confirmed by this measurement

- **First page never scans full history** (US-115 criterion 1): first-page
  log time is flat across 100 → 5,000 commits, consistent with
  `GitCliProvider::commits` already building `git log` with `--skip`/`-n`
  rather than fetching everything (see
  `crates/gitsail-git/src/provider.rs`'s `commits` implementation).
- **Cancellation is responsive and distinct from success** (US-115
  criteria 2–3): the pre-cancelled diff request returns in ~21 ms (the same
  per-process floor as any other call — cancellation was observed before
  the subprocess even had a chance to do meaningful work) as a distinct
  `ErrorCode::Cancelled` error, never a partial `Ok`. `parse_diff`/
  `parse_blame`'s own periodic cancellation checks (added by this epic,
  `crates/gitsail-git/src/provider.rs`) are exercised by
  `parse_diff_is_cancelled_responsively_across_many_files` and
  `parse_blame_is_cancelled_responsively_across_many_lines`
  (`crates/gitsail-git/src/provider.rs` unit tests) — those are
  synthetic-input unit tests, not this fixture-based benchmark, since this
  benchmark's real subprocess exits before parsing ever becomes the
  bottleneck at these scales.
- **Graph layout is cheap relative to the Git call that feeds it**:
  building the first `CommitGraph` page from an already-fetched commit page
  takes microseconds, not milliseconds — negligible next to the ~20 ms
  floor above.
- **Rough memory** (measured via `/proc/self/status` `VmRSS` for the whole
  test process, not isolated per operation — a deliberately "grosseiro"
  measurement, not a profiler): stays in the low single-digit megabytes
  across all three scales, growing only modestly (2.9 → 3.7 MB resident)
  from small to large. No scale here shows unbounded or history-size-scaled
  growth.

## Baseline-derived regression budgets (not an invented SLA)

These are **derived from the measurements above**, on the environment
recorded above — not numbers chosen ahead of time:

- First-page log/graph, diff, and blame on a repository at or below this
  benchmark's "large" scale (5,000 commits, 3 files/commit) should complete
  within **3× the measured baseline** (~60 ms) on comparable hardware.
  Anything consistently beyond that on the same class of machine is a
  regression signal worth investigating before merging.
- A cancelled query must return in on the order of one poll interval
  (documented above as ~20 ms today), not in proportion to the size of the
  work that was cancelled. A cancelled call taking a large fraction of a
  second or more is a regression in cancellation responsiveness, not just
  raw speed.
- Resident memory for these fixture scales should stay within the same
  order of magnitude measured here (single-digit megabytes for the whole
  process). A multi-scale, multi-hundred-megabyte jump for the same
  operation would indicate the "first page never materializes full
  history" guarantee (US-115 criterion 1) has regressed.

These budgets should be re-measured (and this document updated) whenever the
process-execution boundary (`gitsail-git::runner`) changes materially, or
whenever a new representative benchmark environment becomes available —
per SAD §32, they are a living baseline, never a promise frozen in advance.
