use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicI64},
    usize,
};

use crossbeam_channel::bounded;

use crate::core::worker::{Job, WalkWorker};
use anyhow::{Ok, anyhow};
use crossbeam_deque::{Injector, Stealer, Worker};
use crossbeam_utils::thread::ScopedJoinHandle;

const CHANNEL_ITEMS: usize = 256;

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
        // Global work queue
        let injector = Arc::new(Injector::<Job>::new());

        // Create internal workers
        let mut workers: Vec<Worker<Job>> = Vec::with_capacity(self.num_threads);
        // Create internal stealers
        let mut stealers: Vec<Stealer<Job>> = Vec::with_capacity(self.num_threads);

        // Initialize internal workers and stealers
        for _ in 0..self.num_threads {
            let worker = Worker::new_fifo();
            let stealer = worker.stealer();
            workers.push(worker);
            stealers.push(stealer);
        }

        let stealers = Arc::new(stealers);

        let termination = Arc::new(AtomicI64::new(1));

        // Seed global queue with root job
        let root_job = Job::new(root, 0);
        injector.push(root_job);

        // Spawn workers
        let result = crossbeam_utils::thread::scope(|s| {
            let mut handles: Vec<ScopedJoinHandle<'_, anyhow::Result<()>>> = Vec::new();
            for (id, worker) in workers.into_iter().enumerate() {
                let (send_channel, recv_channel) = bounded::<Job>(CHANNEL_ITEMS);
                let mut walk_walker = WalkWorker::new(
                    id,
                    worker,
                    stealers.clone(),
                    injector.clone(),
                    self.num_threads,
                    self.follow_symlinks,
                    self.max_depth,
                    send_channel,
                );
                let termination = termination.clone();
                let worker_handle = s.spawn(move |_| walk_walker.run_loop(termination));
                let processing_handle = s.spawn(move |_| {
                    for job in recv_channel.iter() {
                        println!("File: {}", job.path.display());
                    }
                    Ok(())
                });
                handles.push(worker_handle);
                handles.push(processing_handle);
            }
            // Wait for all workers and collect errors
            for handle in handles {
                if let Err(err) = handle.join() {
                    log::warn!("Worker thread panicked: {:?}", err);
                }
            }
        });
        result.map_err(|e| anyhow!("Thread scope execution failed: {:?}", e))?;
        Ok(())
    }
}
