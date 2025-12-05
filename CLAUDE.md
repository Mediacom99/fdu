# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Role and Workflow

**IMPORTANT:** Claude Code acts as a **code reviewer and advisor** for this project. The user writes the code, and Claude:
- Reviews code for correctness, performance, and best practices
- Provides technical analysis and recommendations
- Evaluates design tradeoffs and alternative approaches
- Answers questions about architecture and implementation details

**DO NOT write or modify code unless explicitly requested by the user.**

## Important: Read Documentation First

**Before starting work, read these documentation files:**

1. **`docs/conversation-summary.md`** - Complete overview of codebase analysis, identified problems, and development priorities
2. **`docs/termination-detection-discussion.md`** - Technical design for the worker termination algorithm (current critical task)

These files contain essential context about design decisions, known issues, and the current development focus.

## Project Overview

`fdu` is a high-performance disk usage analyzer written in Rust, designed to be significantly faster than traditional `du` utilities. It leverages parallel filesystem traversal using work-stealing and crossbeam primitives.

## Build and Development Commands

```bash
# Build the project
cargo build

# Build optimized release binary
cargo build --release

# Run the application (default: analyze current directory)
cargo run

# Run with specific path
cargo run -- /path/to/analyze

# Run with debug logging
FDU_LOG=debug cargo run -- /path/to/analyze

# Run with trace-level logging
FDU_LOG=trace cargo run -- /path/to/analyze

# Run with custom thread count
cargo run -- /path/to/analyze -j 16

# Run tests (if any)
cargo test
```

## Core Architecture

The application uses a **producer-consumer** architecture with work-stealing for parallel filesystem traversal:

### Walker (src/core/walker.rs)
- **Multithreaded**: Entry point that orchestrates the parallel walk
- Creates a global work queue (`Injector`) and per-worker local queues (`Worker`)
- Spawns N worker threads using `crossbeam_utils::thread::scope`
- Seeds the global queue with the root path as the initial `Job`
- Collects results from all workers after completion

### Worker (src/core/worker.rs)
- **WalkWorker**: Individual worker thread that processes filesystem entries
- **Work-stealing strategy**:
  1. Pop from local queue first
  2. Steal batch from global queue
  3. Steal from other workers' queues if both are empty
- **Job processing**:
  - Directories: Read entries and push new directory jobs to the global queue
  - Files: Process metadata immediately (count blocks/size)
- **Termination detection**: Currently has race conditions (see `docs/termination-detection-discussion.md` for detailed analysis and fix)
- Tracks statistics: dirs_processed, files_processed, errors_count, total_blocks

### Work Distribution
- **Job struct**: Represents a path to process, with depth tracking and parent reference
- Directories are distributed across workers via the global queue (work-stealing balances load)
- Files are processed inline by the worker that discovered them
- Uses `local_work_delta` to track work produced vs consumed (for future termination optimization)

### Configuration System (src/config.rs)
Configuration is structured into sub-configs:
- **OutputConfig**: Display options (sort, format, filters)
- **FilterConfig**: Regex patterns for include/exclude
- **TraverseConfig**: Depth limits, symlink handling, filesystem boundaries
- **PerformanceConfig**: Thread count, caching, buffer sizes

All configs are derived from CLI args via `Config::from_cli()`.

### Special File Handling
The `is_special_file()` function (src/core/worker.rs:285) skips block/char devices, FIFOs, sockets, and symlinks when calculating disk usage to avoid double-counting or errors.

## Key Implementation Details

### Block Size Calculation
- Uses `metadata.blocks()` from Unix metadata extensions
- Blocks are 512 bytes on Linux
- Final size: `total_blocks * 512` bytes

### Logging
- Uses `logforth` for structured logging with fastrace integration
- Environment variable `FDU_LOG` controls log level (defaults to Debug in debug builds, Warn in release)
- Custom colored layout in src/fdu/main.rs

### Compiler Optimizations
The `Cargo.toml` release profile is heavily optimized:
- LTO enabled (link-time optimization)
- Single codegen unit for maximum optimization
- Symbols stripped
- opt-level = 3

## Current Limitations (from TODO.md)

### Walking
- Special Linux files handling incomplete (sockets, devices)
- Symlinks and hard links not fully handled
- Filtering (regex, glob, size ranges) not fully implemented

### Processing
- Sorting by time (accessed/modified/created) not implemented
- Progress bar not implemented
- Many CLI options parsed but not yet used in core logic

## Critical Issues & Current Focus

### PRIORITY 1: Fix Termination Detection (BLOCKING)
The worker termination logic (src/core/worker.rs:166-171) has race conditions. See `docs/termination-detection-discussion.md` for:
- Detailed problem analysis
- Recommended solution: Aggressive local batching with exponential backoff
- Implementation approach

**This must be fixed before other work can proceed reliably.**

### PRIORITY 2: Missing Processor Component
The README describes a two-part architecture (Walker + Processor), but only the Walker exists. The Processor component needs to:
- Receive file metadata from walker via channels
- Reconstruct directory hierarchy using dashmap
- Aggregate sizes per directory
- Support sorting, filtering, and output formatting

### PRIORITY 3: Disconnected Config System
The extensive CLI and Config system is ~90% unused. Only `cli.threads` and `cli.paths[0]` are currently wired up to the walker.

## Notes

- Edition 2024 is used (Cargo.toml:4)
- See `docs/conversation-summary.md` for complete analysis of all issues and development roadmap
