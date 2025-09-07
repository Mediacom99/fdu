use std::{
    fs,
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::PathBuf,
    sync::{Arc, atomic::AtomicI64},
    usize,
};

use crossbeam_channel::{Receiver, bounded};

use crate::core::worker::{Job, WalkWorker};
use anyhow::{Ok, Result, anyhow};
use crossbeam_deque::{Injector, Stealer, Worker};
use crossbeam_utils::thread::ScopedJoinHandle;

const CHANNEL_ITEMS: usize = 512;

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
        let mut total_size: u64 = 0;
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
        let root_job = Job::new(root, None, 0, true);
        injector.push(root_job);

        // Spawn workers
        let result = crossbeam_utils::thread::scope(|s| {
            let mut handles: Vec<ScopedJoinHandle<'_, anyhow::Result<()>>> = Vec::new();
            let mut proc_handles: Vec<ScopedJoinHandle<'_, anyhow::Result<u64>>> = Vec::new();
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
                let processing_handle = s.spawn(move |_| process_worker(id, recv_channel));
                handles.push(worker_handle);
                proc_handles.push(processing_handle);
            }
            // Wait for all workers and collect errors
            for handle in handles {
                if let Err(err) = handle.join() {
                    log::warn!("Worker thread panicked: {:?}", err);
                }
            }
            for handle in proc_handles {
                match handle.join() {
                    Result::Ok(size) => {
                        if let Result::Ok(size) = size {
                            total_size += size;
                        }
                    }
                    Err(err) => log::warn!("Processor thread panicked: {:?}", err),
                }
            }
        });
        println!(
            "Total size: {}",
            humansize::format_size(total_size, humansize::DECIMAL)
        );
        result.map_err(|e| anyhow!("Thread scope execution failed: {:?}", e))?;
        Ok(())
    }
}

fn process_worker(id: usize, recv_channel: Receiver<Job>) -> Result<u64> {
    // let mut biggest_path: Option<PathBuf> = None;
    // let mut biggest_size: u64 = 0;
    let mut total_size: u64 = 0;

    let mut job_iter = recv_channel.iter();

    loop {
        let job_buffer: Vec<Job> = job_iter.by_ref().take(CHANNEL_ITEMS).collect();
        log::trace!("Processor {} started on job buffer", id);

        if job_buffer.is_empty() {
            log::trace!("Empty jobs buffer, exiting processing loop...");
            break;
        }

        job_buffer.iter().for_each(|job| {
            match job.path.symlink_metadata() {
                Result::Ok(metadata) => {
                    if !is_special_file(&metadata.file_type()) {
                        let entry_size = metadata.blocks() * 512;
                        total_size += entry_size;
                        // if entry_size > biggest_size {
                        //     biggest_size = entry_size;
                        //     biggest_path = Some(job.path.clone());
                        // }
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
        });
    }
    // if let Some(path) = biggest_path {
    //     println!(
    //         "Size: {}\n\tpath:{}",
    //         humansize::format_size(biggest_size, humansize::DECIMAL),
    //         path.display()
    //     );
    // }
    Ok(total_size)
}

fn is_special_file(file_type: &fs::FileType) -> bool {
    file_type.is_block_device()
        || file_type.is_char_device()
        || file_type.is_fifo()
        || file_type.is_socket()
        || file_type.is_symlink()
}
