use anyhow::anyhow;
use crossbeam_channel::Sender;
use crossbeam_deque::{Injector, Stealer, Worker};
use fastrace::prelude::*;
use std::{
    fs::{self},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
    time::Duration,
    usize,
};

/// A directory path with its depth relative to root item
pub struct Job {
    pub path: PathBuf,
    pub parent: Option<PathBuf>,
    pub depth: usize,
    pub is_dir: bool,
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

    send_channel: Sender<Job>,

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
        send_channel: Sender<Job>,
    ) -> Self {
        Self {
            id,
            inner,
            injector,
            stealers,
            num_workers: num_threads,
            follow_symlinks,
            max_depth,
            send_channel,
            local_work_delta: 0,
            dirs_processed: 0,
            files_processed: 0,
            errors_count: 0,
        }
    }

    pub fn run_loop(&mut self, termination: Arc<AtomicI64>) -> anyhow::Result<()> {
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
                // Or steal from global queue equally between workers
                .or_else(|| {
                    std::iter::repeat_with(|| {
                        self.injector
                            .steal_batch_and_pop(
                                &self.inner,
                                // (&self.injector.len() / self.num_workers).max(1),
                            )
                            //Try stealing a task from other thread
                            .or_else(|| self.stealers.iter().map(|s| s.steal()).collect())
                    })
                    .find(|s| !s.is_retry())
                    .and_then(|s| return s.success())
                });
            //Or steal from busiest worker

            match task {
                Some(item) => {
                    idle_cycles = 0;
                    if let Err(_) = self.process_job(&item) {
                        self.errors_count += 1;
                    };
                }
                None => {
                    //TODO: Here I sync local worker load with global counter
                    //it must finish because at worst the work is distributed perfectly
                    //and they sync only at the end

                    termination.fetch_add(self.local_work_delta, Ordering::AcqRel);
                    self.local_work_delta = 0;

                    if termination.load(Ordering::Acquire) == 0 {
                        log::trace!(
                            "Worker #{} terminating: dirs: {}, files: {}, errors: {}",
                            self.id,
                            self.dirs_processed,
                            self.files_processed,
                            self.errors_count,
                        );
                        break;
                    }

                    // Exponential backoff
                    idle_cycles += 1;
                    if idle_cycles < 10 {
                        std::hint::spin_loop();
                    } else if idle_cycles < 100 {
                        std::thread::yield_now();
                    } else {
                        std::thread::sleep(Duration::from_micros(10));
                    }
                }
            }
        }
        Ok(())
    }

    //TODO: update to use only two worker-local buffers:
    //1. current_work_item_buffer: used to construct work item from entries
    //2. work_items_buffer: buffer that holds new WorkItems to distribute
    fn process_job(&mut self, job: &Job) -> anyhow::Result<()> {
        // Check max depth
        if let Some(max) = self.max_depth {
            if job.depth > max {
                return Ok(());
            }
        }

        // Consume a job from queue
        self.local_work_delta -= 1;
        self.dirs_processed += 1;

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
                                    match self.send_channel.send(new_job) {
                                        Ok(()) => {}
                                        Err(error) => {
                                            log::trace!(
                                                "Worker #{}: Channel receiver error: {}",
                                                self.id,
                                                error
                                            );
                                        }
                                    };
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

        Ok(())
    }
}
