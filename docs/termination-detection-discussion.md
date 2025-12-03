# Walker Termination Detection - Design Discussion

## Context

We're implementing a robust termination detection algorithm for the parallel filesystem walker in `fdu`. The current implementation (src/core/worker.rs:166-171) has race conditions, and we need an approach that:

1. Is **correct** - no premature or delayed termination
2. Minimizes **contention** - avoids excessive futex/atomic operations that kill performance
3. Is **simple** - easy to understand and maintain

## The Problem

The naive approach of checking if all queues are empty has a race:
- Worker A sees all queues empty
- Worker B is processing a directory that's about to spawn 1000 jobs
- Worker A exits prematurely

## Infrastructure Already in Place

```rust
// In walker.rs
let termination = Arc::new(AtomicI64::new(1)); // Global work counter

// In worker.rs
local_work_delta: i64  // Tracks: work_produced - work_consumed

// When consuming job:
self.local_work_delta -= 1;

// When producing job:
self.local_work_delta += 1;
```

The idea: When `global_counter == 0` AND queues are empty, total work produced == total work consumed → safe to terminate.

## Key Insight: Contention is the Enemy

Frequent atomic operations on the shared `termination` counter will create massive contention and kill performance via futex syscalls. The solution must minimize access to shared state.

## Scalability Analysis

### What Doesn't Scale Well

1. **Sync on every idle check** - Too much contention
2. **Active workers counter** - Still gets hammered on idle/active transitions
3. **Epoch-based (for this workload)** - Work is too irregular; hard to pick good epoch boundaries

### What Scales Best for Work-Stealing

#### Option 1: Aggressive Local Batching with Exponential Backoff ⭐ RECOMMENDED

Only sync `local_work_delta` after significant idle time:

```rust
const SYNC_THRESHOLD: i64 = 1000;  // or higher

None => {
    idle_cycles += 1;

    // Exponential backoff - reduce contention
    if idle_cycles < 10 {
        std::hint::spin_loop();  // Spin briefly
    } else if idle_cycles < 1000 {
        std::thread::yield_now();  // Yield to scheduler
    } else if idle_cycles == 1000 {
        // Only sync after serious idleness
        if self.local_work_delta != 0 {
            termination.fetch_add(self.local_work_delta, Ordering::AcqRel);
            self.local_work_delta = 0;
        }
    } else if idle_cycles > 5000 {
        // Final termination check
        if termination.load(Ordering::Acquire) == 0
            && self.inner.is_empty()
            && self.injector.is_empty()
            && self.stealers.iter().all(|s| s.len() == 0)
        {
            break;
        }
        idle_cycles = 1001; // Stay in final check phase
    }
}

// When work is found:
Some(item) => {
    idle_cycles = 0;  // Reset
    // ... process work
}
```

**Why this works:**
- Workers that find work quickly never sync (most common case)
- Only sync after 1000 idle cycles (significant delay)
- Exponential backoff reduces queue contention too
- Simple to implement (~20 lines)
- Provably correct

#### Option 2: Threshold-Based Work Batching

Only sync when local delta is large:

```rust
// After producing/consuming work:
if self.local_work_delta.abs() > SYNC_THRESHOLD {
    termination.fetch_add(self.local_work_delta, Ordering::AcqRel);
    self.local_work_delta = 0;
}
```

Good for workloads with large directories. Can be combined with Option 1.

#### Option 3: Hierarchical/Tree-Based (Most Scalable, Most Complex)

Workers arranged in a tree structure - only coordinate with 2-3 neighbors instead of all N workers. This is what truly scalable distributed systems use (e.g., MPI, large-scale parallel processing).

**Overkill for this use case** - the complexity doesn't justify the gains for typical filesystem walking.

## Recommendation

**Implement Option 1: Aggressive Local Batching with Exponential Backoff**

Benefits:
- Dramatically reduces atomic operations (only after 1000+ idle cycles)
- Exponential backoff reduces steal attempts and queue contention
- Simple and maintainable
- Proven approach in work-stealing literature

Trade-offs:
- Slight delay in termination detection (workers idle for ~5000 cycles before exiting)
- In practice, this is microseconds and doesn't matter

## Implementation Notes

1. Tune `SYNC_THRESHOLD` based on workload (start with 1000)
2. Consider combining with work batching: batch directory entries before pushing to queue
3. The exponential backoff helps both termination AND reduces contention on work queues
4. Could add metrics to track sync frequency in debug builds

## References

- Work-stealing deques: Chase & Lev, "Dynamic Circular Work-Stealing Deque" (2005)
- Distributed termination detection: Dijkstra, Scholten "Termination Detection for Diffusing Computations" (1980)
- Crossbeam documentation on work-stealing patterns

## Next Steps

1. Implement the backoff-based approach in worker.rs
2. Add configuration for sync threshold (start with 1000, tune if needed)
3. Test with large directory trees to verify correctness
4. Profile to confirm reduced atomic contention
