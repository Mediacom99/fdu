use crate::cli::{Cli, SortField};
use crate::utils;
use anyhow::{Context, Ok, Result};
use regex::Regex;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub paths: Vec<PathBuf>,
    pub output_config: OutputConfig,
    pub filter_config: FilterConfig,
    pub traverse_config: TraverseConfig,
    pub performance_config: PerformanceConfig,
}

impl Config {
    pub fn from_cli(cli: &Cli) -> Result<Self> {
        let paths: Vec<PathBuf> = cli
            .paths
            .clone()
            .into_iter()
            .filter(|path| path.exists())
            .collect();

        if paths.is_empty() {
            anyhow::bail!(
                "Given paths do not exist: {}",
                paths
                    .into_iter()
                    .map(|path| { path.display().to_string() })
                    .collect::<String>()
            )
        }

        Ok(Config {
            paths,
            output_config: OutputConfig::from_cli(cli)?,
            filter_config: FilterConfig::from_cli(cli)?,
            traverse_config: TraverseConfig::from_cli(cli)?,
            performance_config: PerformanceConfig::from_cli(cli)?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct OutputConfig {
    pub all: bool,
    pub dirs_only: bool,
    pub files_only: bool,
    pub apparent_size: bool,
    pub show_time: bool,
    pub sort_field: Option<SortField>,
    pub reverse: bool,
    pub threshold: Option<u64>,
    pub total: bool,
    pub summarize: bool,
}

impl OutputConfig {
    fn from_cli(cli: &Cli) -> Result<Self> {
        // Parse threshold (human readable size) into number of bytes
        let threshold = if let Some(t) = &cli.threshold {
            Some(utils::parse_size(t).context("Invalid threshold size")?)
        } else {
            None
        };

        Ok(OutputConfig {
            all: cli.all,
            dirs_only: cli.dirs_only,
            files_only: cli.files_only,
            apparent_size: cli.apparent_size,
            show_time: cli.show_time,
            sort_field: cli.sort,
            reverse: cli.reverse,
            threshold,
            total: cli.total,
            summarize: cli.summarize,
        })
    }
}

#[derive(Debug, Clone)]
pub struct FilterConfig {
    pub exclude_patterns: Vec<Regex>,
    pub include_patterns: Vec<Regex>,
    pub exclude_caches: bool,
}

impl FilterConfig {
    fn from_cli(cli: &Cli) -> Result<Self> {
        let include_patterns = cli
            .include_patterns
            .iter()
            .map(|p| Regex::new(p).with_context(|| format!("Invalid include pattern: {p}")))
            .collect::<Result<Vec<_>>>()?;

        let exclude_patterns = cli
            .exclude_patterns
            .iter()
            .map(|p| Regex::new(p).with_context(|| format!("Invalid exclude pattern: {p}")))
            .collect::<Result<Vec<_>>>()?;

        //TODO: load patterns from file

        Ok(FilterConfig {
            exclude_patterns,
            include_patterns,
            exclude_caches: cli.exclude_caches,
        })
    }
}

#[derive(Debug, Clone)]
pub struct TraverseConfig {
    pub max_depth: Option<usize>,
    pub min_depth: Option<usize>,
    pub follow_symlinks: bool,
    pub cross_filesystems: bool,
    pub count_hard_links: bool,
}

impl TraverseConfig {
    fn from_cli(cli: &Cli) -> Result<Self> {
        if let Some(max_depth) = cli.max_depth {
            anyhow::ensure!(max_depth > 0, "Max depth must be greater than 0");
            anyhow::ensure!(max_depth <= 1000, "Max depth too large (maximum: 1000)");
        };
        if let Some(min_depth) = cli.min_depth {
            anyhow::ensure!(min_depth <= 1000, "Min depth too large (maximum: 1000)");
        }
        Ok(TraverseConfig {
            max_depth: cli.max_depth,
            min_depth: cli.min_depth,
            follow_symlinks: cli.dereference,
            cross_filesystems: !cli.one_file_system,
            count_hard_links: cli.count_links,
        })
    }
}

#[derive(Debug, Clone)]
pub struct PerformanceConfig {
    pub threads: usize,
    pub batch_size: usize,
    pub channel_buffer: usize,
    pub cache_size_bytes: usize,
    pub use_cache: bool,
    pub buffer_errors: bool,
}

impl PerformanceConfig {
    fn from_cli(cli: &Cli) -> Result<Self> {
        let threads = if cli.threads == 0 {
            num_cpus::get()
        } else {
            anyhow::ensure!(
                cli.threads <= 1000,
                "Thread count too large (maximum: 1000)"
            );
            cli.threads
        };
        let cache_size_mb = cli.cache_size_mb.min(10_000); //cap at 10GB
        let cache_size_bytes = cache_size_mb.saturating_mul(1024 * 1024);

        Ok(PerformanceConfig {
            threads,
            batch_size: 64,
            channel_buffer: 1000,
            cache_size_bytes,
            use_cache: !cli.no_cache,
            buffer_errors: cli.buffer_errors,
        })
    }
}
