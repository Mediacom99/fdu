use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicI64},
};

use crate::core::worker::{Job, WalkWorker, WorkerResult};
use anyhow::anyhow;
use crossbeam_deque::{Injector, Stealer, Worker};
use crossbeam_utils::thread::ScopedJoinHandle;
use humansize::Kilo;

pub struct Multithreaded {
    num_threads: usize,
    follow_symlinks: bool,
    max_depth: Option<usize>,
    // _min_depth: Option<usize>,
}

impl Multithreaded {
    pub fn new(num_threads: usize) -> Self {
        Self {
            num_threads,
            follow_symlinks: false,
            max_depth: None,
        }
    }

    pub fn walk(&self, root: PathBuf) -> anyhow::Result<()> {
        let mut total_blocks: u64 = 0;
        // Global work queue
        let global_injector = Arc::new(Injector::<Job>::new());

        // Create internal workers
        let mut workers: Vec<Worker<Job>> = Vec::with_capacity(self.num_threads);
        // Create internal stealers
        let mut stealers: Vec<Stealer<Job>> = Vec::with_capacity(self.num_threads);

        // Initialize internal workers and stealers
        for _ in 0..self.num_threads {
            let worker = Worker::new_lifo();
            let stealer = worker.stealer();
            workers.push(worker);
            stealers.push(stealer);
        }

        let stealers = Arc::new(stealers);

        let global_job_counter = Arc::new(AtomicI64::new(1));

        // Seed global queue with a root job
        let mut root_job = Job::new(root.clone(), None, 0, true);
        if let Ok(metadata) = root.symlink_metadata() {
            if metadata.is_file() {
                root_job.is_dir = false;
            }
        }
        global_injector.push(root_job);

        // Spawn workers
        let result = crossbeam_utils::thread::scope(|s| {
            let mut handles: Vec<ScopedJoinHandle<'_, anyhow::Result<WorkerResult>>> = Vec::new();
            for (id, worker) in workers.into_iter().enumerate() {
                let mut walk_walker = WalkWorker::new(
                    id,
                    worker,
                    stealers.clone(),
                    global_injector.clone(),
                    self.num_threads,
                    self.follow_symlinks,
                    self.max_depth,
                );
                let gjc_clone = global_job_counter.clone();
                let worker_handle = s.spawn(move |_| walk_walker.run_loop(gjc_clone));
                handles.push(worker_handle);
            }

            // Wait for all workers and collect errors
            for handle in handles {
                match handle.join() {
                    Ok(ok) => {
                        if let Ok(worker_result) = ok {
                            total_blocks += worker_result.total_blocks;
                        } else {
                            log::warn!("Failed to get worker result");
                        }
                    }
                    Err(err) => {
                        log::warn!("Worker thread panicked: {:?}", err);
                    }
                }
            }
        });
        println!(
            "✅ Disk usage: {}",
            humansize::format_size(total_blocks * 512, humansize::DECIMAL),
        );
        result.map_err(|e| anyhow!("Thread scope execution failed: {:?}", e))?;
        Ok(())
    }
}
