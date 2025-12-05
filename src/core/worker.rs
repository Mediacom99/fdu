use anyhow::anyhow;
use crossbeam_deque::{Injector, Stealer, Worker};
use fastrace::prelude::*;
use std::{
    fs::{self},
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
};

/// A directory path with its depth relative to root item
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
    /// TODO: this is what I have to sync globally when idle
    /// syncing means I set the add to global_count the local_delta
    /// and I set the local_delta to zero
    /// The global counter is zero only when each worker has no net work coming out of it
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
            //Get work with new strategy
            let task = self
                //Pop from local
                .inner
                .pop()
                .inspect(|task| {
                    log::trace!(
                        "Worker {} popped task from local queue: {}",
                        self.id,
                        task.path.display()
                    )
                })
                // Or steal from the global queue equally between workers
                .or_else(|| {
                    std::iter::repeat_with(|| {
                        let global_steal = self.injector.steal_batch_and_pop(
                            &self.inner,
                            // (&self.injector.len() / self.num_workers).max(1),
                        );
                        if global_steal.is_success() {
                            log::trace!("Worker {} stole from global queue", self.id);
                        }
                        //Try stealing a task from another thread
                        let direct_steal = global_steal
                            .or_else(|| self.stealers.iter().map(|s| s.steal()).collect());
                        if direct_steal.is_success() {
                            log::trace!("Worker {} stole from victim thread", self.id);
                        }
                        direct_steal
                    })
                    .find(|s| !s.is_retry())
                    .and_then(|s| return s.success())
                });

            match task {
                Some(item) => {
                    idle_cycles = 0;
                    if let Err(_) = self.process_job(&item) {
                        self.errors_count += 1;
                    };
                }
                None => {
                    idle_cycles += 1;
                    if idle_cycles < 10 {
                        //SPIN: light spinning for 10 cycles, maybe there will be a burst of work from other thread
                        std::hint::spin_loop();
                        continue;
                    } else if idle_cycles < 1000 {
                        //YIELD: because work might appear soon but not instantly, give timeslice to scheduler
                        std::thread::yield_now();
                        continue;
                    } else if idle_cycles == 1000 {
                        //SYNC: local work delta with global counter after 1000 cycles we're probably near termination
                        if self.local_work_delta != 0 {
                            global_job_counter.fetch_add(self.local_work_delta, Ordering::AcqRel);
                            self.local_work_delta = 0;
                        }
                        continue;
                    } else if idle_cycles < 5000 { //keep yielding and check for work
                        //WAIT
                        //Final termination check, keep yielding and check for work
                        std::thread::yield_now();
                        continue;
                    } else {
                        //TERMINATE
                        //Final termination check
                        if global_job_counter.load(Ordering::Acquire) == 0
                            && self.inner.is_empty() //the local queue is empty
                            && self.injector.is_empty() //the global queue is empty
                            && self.stealers.iter().all(|s| s.len() == 0) { //stealers are empty
                            break;
                        }
                        idle_cycles = 1001; //stay in the synced phase
                        continue;
                    }
                }
            }
        }
        anyhow::Ok(WorkerResult::new(&self))
    }

    //TODO: update to use only two worker-local buffers:
    //1. current_work_item_buffer: used to construct work item from entries
    //2. work_items_buffer: buffer that holds new WorkItems to distribute
    fn process_job(&mut self, job: &Job) -> anyhow::Result<()> {
        // Check max depth
        if let Some(max) = self.max_depth {
            if job.depth > max {
                return anyhow::Ok(());
            }
        }

        // Consume a job from queue
        self.local_work_delta -= 1;
        self.dirs_processed += 1;

        // Short path if root is file
        if !job.is_dir {
            self.files_processed += 1;
            self.process_file(&job)?;
            return anyhow::Ok(());
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
                                    // Send to global queue or batch and then send
                                    new_job.is_dir = true;
                                    self.injector.push(new_job);
                                    self.local_work_delta += 1;
                                } else {
                                    self.files_processed += 1;
                                    self.process_file(&new_job)?;
                                }
                            }
                        }
                        Err(err) => {
                            self.errors_count += 1;
                            log::warn!("Failed to read directory entry, skipping: {}", err)
                        }
                    }
                }
            }
            Err(err) => {
                // log::warn!("Failed to read directory {:?}: {}", job.path, err);
                return Err(anyhow!(
                    "Failed to read directory {:?}, exiting job, err: {}",
                    job.path,
                    err
                ));
            }
        }

        anyhow::Ok(())
    }

    fn process_file(&mut self, job: &Job) -> anyhow::Result<()> {
        match job.path.symlink_metadata() {
            Result::Ok(metadata) => {
                if !is_special_file(&metadata.file_type()) {
                    self.total_blocks += metadata.blocks();
                }
            }
            Err(err) => {
                log::warn!(
                    "Failed to read metadata for file: {}, error: {}",
                    job.path.display(),
                    err
                );
            }
        };
        anyhow::Ok(())
    }
}

fn is_special_file(file_type: &fs::FileType) -> bool {
    file_type.is_block_device()
        || file_type.is_char_device()
        || file_type.is_fifo()
        || file_type.is_socket()
        || file_type.is_symlink()
}
