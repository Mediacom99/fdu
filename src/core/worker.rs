use crossbeam_deque::{Injector, Steal, Stealer, Worker};
use fastrace::prelude::*;
use std::{
    fs::{self},
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
    time::Duration,
};

/// A directory path with its depth relative to the root item
pub struct Job {
    pub path: PathBuf,
    pub parent: Option<PathBuf>,
    pub depth: usize,
    pub is_dir: bool,
}

pub struct WorkerResult {
    pub total_blocks: u64,
}

impl WorkerResult {
    pub fn new(worker: &WalkWorker) -> Self {
        Self {
            total_blocks: worker.total_blocks,
        }
    }
}

impl Job {
    pub fn new(path: PathBuf, parent: Option<PathBuf>, depth: usize, is_dir: bool) -> Self {
        Self {
            path,
            parent,
            depth,
            is_dir,
        }
    }
}

/// Worker state
pub struct WalkWorker {
    id: usize,

    /// Internal crossbeam worker
    inner: Worker<Job>,

    /// Shared crossbeam global queue injector
    injector: Arc<Injector<Job>>,

    /// Shared vector of crossbeam stealers
    stealers: Arc<Vec<Stealer<Job>>>,

    /// Configuration
    num_workers: usize,
    follow_symlinks: bool,
    max_depth: Option<usize>,

    /// Local work delta (work produced - work consumed)
    /// This is what has to be synced globally when idle.
    /// Syncing means adding to global_count the local_delta
    /// and setting the local_delta to zero.
    /// The global counter is zero only when each worker has no net work coming out of it.
    local_work_delta: i64,

    /// Statistics
    dirs_processed: usize,
    files_processed: usize,
    errors_count: usize,

    /// Data that can be calculated walking
    total_blocks: u64,
}

impl WalkWorker {
    pub fn new(
        id: usize,
        inner: Worker<Job>,
        stealers: Arc<Vec<Stealer<Job>>>,
        injector: Arc<Injector<Job>>,
        num_threads: usize,
        follow_symlinks: bool,
        max_depth: Option<usize>,
    ) -> Self {
        Self {
            id,
            inner,
            injector,
            stealers,
            num_workers: num_threads,
            follow_symlinks,
            max_depth,
            local_work_delta: 0,
            dirs_processed: 0,
            files_processed: 0,
            errors_count: 0,
            total_blocks: 0,
        }
    }

    /// Try to get work: local queue -> global queue -> steal from victims
    fn find_work(&self) -> Option<Job> {
        // 1. Try popping from the local queue first (the fastest path)
        if let Some(job) = self.inner.pop() {
            log::trace!(
                "Worker {} popped from local queue: {}",
                self.id,
                job.path.display()
            );
            return Some(job);
        }

        // 2. Try stealing from other workers
        if let Some(job) = self.steal_from_victims() {
            return Some(job);
        }

        // 3. Try stealing from the global queue
        self.steal_from_global()
    }

    /// Steal from the global queue with adaptive batching
    fn steal_from_global(&self) -> Option<Job> {
        // Calculate a fair batch size based on queue length
        let batch_size = (self.injector.len() / self.num_workers)
            .max(1) // Always try to steal at least 1
            .min(32); // Cap at 32 to avoid hogging

        loop {
            match self
                .injector
                .steal_batch_with_limit_and_pop(&self.inner, batch_size)
            {
                Steal::Success(job) => {
                    log::trace!("Worker {} stole batch from global queue", self.id);
                    return Some(job);
                }
                Steal::Empty => {
                    // Global queue is definitely empty
                    return None;
                }
                Steal::Retry => {
                    // Race condition detected, retry immediately
                    continue;
                }
            }
        }
    }

    /// Try stealing from other workers' queues
    fn steal_from_victims(&self) -> Option<Job> {
        // Try each worker's queue in sequence
        for (i, stealer) in self.stealers.iter().enumerate() {
            // Skip stealing from self
            if i == self.id {
                continue;
            }

            match stealer.steal() {
                Steal::Success(job) => {
                    log::trace!("Worker {} stole from victim", self.id);
                    return Some(job);
                }
                Steal::Empty => {
                    // This victim has nothing, try next
                    continue;
                }
                Steal::Retry => {
                    // Race condition, try the next victim
                    // (could retry the same victim, but trying next is simpler)
                    continue;
                }
            }
        }
        // All victims were empty or had races
        None
    }

    /// Check if this worker should terminate
    #[inline]
    fn should_terminate(&self, global_job_counter: &Arc<AtomicI64>) -> bool {
        global_job_counter.load(Ordering::Acquire) == 0
            && self.inner.is_empty()
            && self.injector.is_empty()
            && self.stealers.iter().all(|s| s.len() == 0)
    }

    pub fn run_loop(&mut self, global_job_counter: Arc<AtomicI64>) -> anyhow::Result<WorkerResult> {
        // Setup fastrace span for this function
        #[cfg(debug_assertions)]
        let (_worker_span, _guard) = {
            let worker_span = Span::root("worker_loop", SpanContext::random());
            let guard = worker_span.set_local_parent();
            worker_span.add_property(|| ("worker_id", self.id.to_string()));
            (worker_span, guard) // Return both to keep them alive
        };

        let mut idle_cycles = 0;

        loop {
            // Try to find work using the three-tier strategy
            match self.find_work() {
                Some(job) => {
                    idle_cycles = 0; // Reset idle counter

                    if let Err(_) = self.process_job(&job) {
                        self.errors_count += 1;
                    }
                }
                None => {
                    // No work found, enter an exponential backoff sequence
                    idle_cycles += 1;
                    match idle_cycles {
                        // Phase 1: Light spinning (1-10 cycles)
                        1..=10 => {
                            std::hint::spin_loop();
                        }
                        // Phase 3: Sync local work delta
                        11 => {
                            if self.local_work_delta != 0 {
                                global_job_counter
                                    .fetch_add(self.local_work_delta, Ordering::AcqRel);
                                self.local_work_delta = 0;
                            }
                        }
                        12..=50 => {
                            // Yield every 10 cycles
                            if idle_cycles % 10 == 0 {
                                std::thread::yield_now();
                            }
                            if self.should_terminate(&global_job_counter) {
                                log::info!(
                                    "Worker {} terminating: dirs={}, files={}, errors={}",
                                    self.id,
                                    self.dirs_processed,
                                    self.files_processed,
                                    self.errors_count
                                );
                                break;
                            }
                        }
                        _ => {
                            if self.should_terminate(&global_job_counter) {
                                log::info!(
                                    "Worker {} terminating: dirs={}, files={}, errors={}",
                                    self.id,
                                    self.dirs_processed,
                                    self.files_processed,
                                    self.errors_count
                                );
                                break;
                            }
                            std::thread::sleep(Duration::from_nanos(500));
                            idle_cycles = 12;
                        }
                    }
                }
            }
        }
        anyhow::Ok(WorkerResult::new(&self))
    }

    fn process_job(&mut self, job: &Job) -> anyhow::Result<(), anyhow::Error> {
        // Check max depth
        if let Some(max) = self.max_depth {
            if job.depth > max {
                return Err(anyhow::anyhow!(
                    "Worker {} has reached max depth: {} > {}",
                    self.id,
                    job.depth,
                    max
                ));
            }
        }

        // Consume a job from the queue
        self.local_work_delta -= 1;

        // Short path if the root path is a file
        if !job.is_dir {
            self.files_processed += 1;
            return self.process_file(&job);
        }

        // Read entries
        match fs::read_dir(&job.path) {
            Ok(entries) => {
                for entry in entries {
                    match entry {
                        Ok(entry) => {
                            if let Some(ft) = entry.file_type().ok() {
                                let parent = entry.path().parent().map(|p| p.to_path_buf());
                                let mut new_job =
                                    Job::new(entry.path(), parent, job.depth + 1, false);
                                if ft.is_dir() {
                                    // Send to local queue
                                    new_job.is_dir = true;
                                    self.inner.push(new_job);
                                    self.local_work_delta += 1;
                                } else {
                                    self.files_processed += 1;
                                    if let Err(_) = self.process_file(&new_job) {
                                        self.errors_count += 1;
                                    }
                                }
                            }
                        }
                        Err(err) => {
                            self.errors_count += 1;
                            log::warn!(
                                "Worker {} failed to read directory entry, skipping: {}",
                                self.id,
                                err
                            );
                        }
                    }
                }
                self.dirs_processed += 1;
                anyhow::Ok(())
            }
            Err(err) => {
                log::warn!(
                    "Worker {} failed to open directory {}: {}",
                    self.id,
                    job.path.display(),
                    err
                );
                Err(err.into())
            }
        }
    }

    fn process_file(&mut self, job: &Job) -> Result<(), anyhow::Error> {
        match job.path.symlink_metadata() {
            Ok(metadata) => {
                if !is_special_file(&metadata.file_type()) {
                    self.total_blocks += metadata.blocks();
                } else {
                    log::warn!(
                        "Worker {} skipped special file: {}",
                        self.id,
                        job.path.display(),
                    );
                }
                anyhow::Ok(())
            }
            Err(err) => {
                log::warn!(
                    "Worker {} failed to read metadata for file: {}, error: {}",
                    self.id,
                    job.path.display(),
                    err
                );
                Err(err.into())
            }
        }
    }
}

fn is_special_file(file_type: &fs::FileType) -> bool {
    file_type.is_block_device()
        || file_type.is_char_device()
        || file_type.is_fifo()
        || file_type.is_socket()
        || file_type.is_symlink()
}
