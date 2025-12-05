use anyhow::Result;
use clap::Parser;
use fdu::{cli, core::walker};
use logforth::{
    append,
    colored::Colorize,
    filter::{EnvFilter, env_filter::EnvFilterBuilder},
};

#[derive(Debug)]
pub struct CustomTextLayout {}
impl CustomTextLayout {
    fn new() -> Self {
        CustomTextLayout {}
    }
}

impl logforth::layout::Layout for CustomTextLayout {
    fn format(
        &self,
        record: &log::Record,
        _diagnostics: &[Box<dyn logforth::Diagnostic>],
    ) -> anyhow::Result<Vec<u8>> {
        let level_str = match record.level() {
            log::Level::Error => "ERROR".red().bold(),
            log::Level::Warn => "WARN".yellow().bold(),
            log::Level::Info => "INFO".green().bold(),
            log::Level::Debug => "DEBUG".blue().bold(),
            log::Level::Trace => "TRACE".purple().bold(),
        };

        // let level_str = match record.level() {
        //     log::Level::Error => "ERROR",
        //     log::Level::Warn => "WARN",
        //     log::Level::Info => "INFO",
        //     log::Level::Debug => "DEBUG",
        //     log::Level::Trace => "TRACE",
        // };

        // let origin_source_file = record.target();

        let formatted = format!("[{}] {}", level_str, record.args());
        Ok(formatted.into_bytes())
    }
}

fn main() -> Result<()> {
    let cli = cli::Cli::parse();
    let filter_builder = EnvFilterBuilder::try_from_env("FDU_LOG").unwrap_or_else(|| {
        let default_level = if cfg!(debug_assertions) {
            log::LevelFilter::Debug
        } else {
            log::LevelFilter::Info
        };
        EnvFilterBuilder::new().filter_level(default_level)
    });

    logforth::builder()
        .dispatch(|d| {
            let dispatch = d
                .filter(EnvFilter::new(filter_builder))
                .append(append::Stderr::default().with_layout(CustomTextLayout::new()));
            // if cli.trace {
            //     dispatch = dispatch.append(append::FastraceEvent::default());
            // }
            dispatch
        })
        .apply();

    log::info!("Starting fdu v{}, threads: {}", env!("CARGO_PKG_VERSION"), cli.threads);
    let multi_walker = walker::Multithreaded::new(cli.threads);
    multi_walker.walk(cli.paths[0].clone())?;
    fastrace::flush();
    Ok(())
}
