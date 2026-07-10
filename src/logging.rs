use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use anyhow::{anyhow, Context, Result};
use tracing::{info, Level};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::filter::filter_fn;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

static LOG_GUARDS: OnceLock<Vec<WorkerGuard>> = OnceLock::new();

pub fn init() {
    if let Err(error) = try_init() {
        eprintln!("failed to initialize logging: {error:?}");
    }
}

fn try_init() -> Result<()> {
    let logs_dir = logs_dir()?;
    fs::create_dir_all(&logs_dir)
        .with_context(|| format!("failed to create logs directory {}", logs_dir.display()))?;

    let info_file = tracing_appender::rolling::never(&logs_dir, "info.log");
    let warn_file = tracing_appender::rolling::never(&logs_dir, "warn.log");
    let error_file = tracing_appender::rolling::never(&logs_dir, "error.log");
    let (info_writer, info_guard) = tracing_appender::non_blocking(info_file);
    let (warn_writer, warn_guard) = tracing_appender::non_blocking(warn_file);
    let (error_writer, error_guard) = tracing_appender::non_blocking(error_file);

    let info_layer = tracing_subscriber::fmt::layer()
        .with_writer(info_writer)
        .with_ansi(false)
        .with_target(false)
        .compact()
        .with_filter(filter_fn(|metadata| *metadata.level() == Level::INFO));
    let warn_layer = tracing_subscriber::fmt::layer()
        .with_writer(warn_writer)
        .with_ansi(false)
        .with_target(false)
        .compact()
        .with_filter(filter_fn(|metadata| *metadata.level() == Level::WARN));
    let error_layer = tracing_subscriber::fmt::layer()
        .with_writer(error_writer)
        .with_ansi(false)
        .with_target(false)
        .compact()
        .with_filter(filter_fn(|metadata| *metadata.level() == Level::ERROR));

    tracing_subscriber::registry()
        .with(info_layer)
        .with(warn_layer)
        .with(error_layer)
        .try_init()
        .context("failed to initialize tracing subscriber")?;

    let _ = LOG_GUARDS.set(vec![info_guard, warn_guard, error_guard]);
    let exe_path = std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "<unknown>".to_string());
    info!(
        pid = std::process::id(),
        exe_path = %exe_path,
        logs_dir = %logs_dir.display(),
        "file logging initialized"
    );
    Ok(())
}

fn logs_dir() -> Result<PathBuf> {
    let exe_path = std::env::current_exe().context("failed to get current executable path")?;
    let app_dir = exe_path
        .parent()
        .ok_or_else(|| anyhow!("failed to resolve executable directory"))?;
    Ok(app_dir.join("logs"))
}
