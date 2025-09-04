# TODO: FDU Performance Optimization Plan

## Goal
Achieve the fastest parallel filesystem walker in Rust, optimized for large-scale filesystems (petabyte+).

## Implementation Status

### ✅ Completed
- [x] Created `walk_balanced.rs` - Phase 1 implementation with work balancing
- [x] Created `walk_distributed.rs` - Phase 2 with distributed termination detection
- [x] Implemented Arc<PathBuf> for shared ownership
- [x] Added thread-local reusable buffers (entry_buffer, work_buffer)
- [x] Implemented steal-half strategy
- [x] Added per-worker load tracking with atomic counters
- [x] Implemented size-based distribution strategies
- [x] Changed work-stealing order: local → steal from busiest → global
- [x] Replaced spin_loop with exponential backoff
- [x] Added tracing with zero-cost feature flag
- [x] Comprehensive error handling with Result types
- [x] Worker statistics tracking (dirs/files processed, errors)

### 🔄 In Progress
- [ ] Testing Phase 1 implementation (`walk_balanced.rs`)
- [ ] Benchmarking against current implementation

## Phase 1: Core Walker Optimizations (Target: 30-40% improvement)

### 1.1 Memory Optimization
- [x] Replace PathBuf cloning with Arc<PathBuf> for shared ownership
- [x] Implement thread-local reusable buffers for directory entries
- [ ] Use SmallVec for directories with <10 entries (stack allocation)

### 1.2 Work Distribution Improvements
- [x] Implement steal-half strategy (steal 50% of victim's queue instead of 1 item)
- [x] Add per-worker load tracking with atomic counters
- [x] Implement size-based distribution strategies:
  - 0-10 entries: Keep local
  - 10-100 entries: Allow stealing
  - 100-1000 entries: Donate 50% to least loaded worker
  - 1000+ entries: Broadcast - split evenly across all workers

### 1.3 Synchronization Optimization
- [x] Change work-stealing order: local → other workers → global
- [x] Replace spin_loop with exponential backoff
- [ ] Phase 2: Distributed termination detection (Conservation of Work)
  - [x] Implemented in `walk_distributed.rs`
  - [ ] Test and validate correctness
  - [ ] Benchmark improvement over atomic counter

### 1.4 Batching Improvements
- [ ] Increase batch size from 32 to 256-512
- [ ] Implement dynamic batching based on directory size
- [ ] Batch channel sends to reduce synchronization

## Phase 2: Termination Detection

### 2.1 Distributed Termination (Conservation of Work)
- [x] Implement WorkAccount structure for tracking work flow
- [x] Add per-worker work accounting (local_work, sent, received)
- [x] Implement equilibrium detection (balanced when sent == received && local == 0)
- [x] Remove atomic counter from hot path
- [x] Add periodic termination checking (not every cycle)
- [ ] Validate correctness with extensive testing

### 2.2 Conditional Metrics System
- [x] Add tracing crate with feature flag
- [x] Implement zero-cost logging macros when disabled
- [x] Track metrics:
  - Work distribution (donate/broadcast events)
  - Steal counts and amounts
  - Worker termination states
  - Error counts per worker

### 2.3 Benchmarking
- [ ] Create synthetic test datasets:
  - Shallow but wide (millions of files in few dirs)
  - Deep but narrow (deeply nested structures)
  - Mixed real-world scenarios
- [ ] Compare against: diskus, du, fd, dua
- [ ] Profile with perf and flamegraph

## Phase 3: Advanced Optimizations

### 3.1 System Call Optimization
- [ ] Use DirEntry metadata to avoid double stat() calls
- [ ] Implement readdir() buffering to reduce syscalls
- [ ] Add posix_fadvise() hints for sequential access

### 3.2 Platform-Specific (Linux)
- [ ] Direct syscall implementation with openat() + getdents64()
- [ ] Investigate io_uring for async I/O
- [ ] NUMA-aware work stealing (prefer same-node cores)

### 3.3 Memory Pool
- [ ] Implement arena allocator for paths
- [ ] String interning for common path prefixes
- [ ] Zero-copy path construction where possible

## Phase 4: Output Pipeline

### 4.1 Channel Design
- [ ] Implement bounded crossbeam channel (capacity ~10000)
- [ ] Design WalkEvent enum:
  ```rust
  enum WalkEvent {
      EnterDir(Arc<PathBuf>, Metadata),
      File(Arc<PathBuf>, Metadata),
      ExitDir(Arc<PathBuf>),
  }
  ```
- [ ] Batch sends (every 100-200 items)

### 4.2 Processor Component
- [ ] Separate thread pool for processing
- [ ] DashMap for concurrent directory reconstruction
- [ ] Streaming output for real-time results

## Performance Targets

- **Current**: ~220-240ms with high variance (matching diskus)
- **Phase 1 Target**: ~150-180ms with low variance
- **Phase 2 Target**: ~120-150ms with metrics
- **Phase 3 Target**: <100ms for standard benchmarks
- **Final Goal**: 20-30% faster than any Rust alternative

## Build Profiles

```toml
[profile.bench]
lto = true
codegen-units = 1
opt-level = 3
debug = false

[profile.release-metrics]
inherits = "release"
debug = true  # For profiling

[features]
default = []
metrics = ["tracing", "tracing-subscriber"]
linux-optimized = []  # Platform specific optimizations
```

## Testing Strategy

1. Unit tests for work distribution logic
2. Integration tests with known directory structures
3. Fuzz testing for edge cases
4. Stress tests on massive synthetic filesystems
5. Real-world tests on actual production filesystems

## Notes

- Priority is large filesystem performance over small directory overhead
- Acceptable to be 5-10ms slower on tiny directories if massive FS gains achieved
- Focus on reducing work distribution variance as primary goal
- All optimizations should be measurable with metrics
