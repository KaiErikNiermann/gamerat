//! `gamerat-daemon` library crate.
//!
//! The binary in `main.rs` is a thin wrapper around [`run`]; everything
//! interesting — D-Bus service, dispatch loop, rule store — lives here
//! so it can be unit-tested without spawning a process.

pub mod dispatch;
pub mod paths;
pub mod rules;
pub mod service;

use std::path::PathBuf;

use anyhow::{Context as _, Result};
use clap::{Parser, ValueEnum};
use gamerat_focus::{FocusBackend as _, FocusStream, SyntheticBackend, WlrForeignToplevelBackend};
use gamerat_ratbag::Service as RatbagService;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::RwLock;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use crate::dispatch::run_dispatch;
use crate::rules::RuleStore;
use crate::service::{AppHandle, DaemonStatus, GameRatService};

/// CLI surface for `gamerat-daemon`.
#[derive(Debug, Parser)]
#[command(name = "gamerat-daemon", version, about)]
pub struct Args {
    /// Talk to `ratbagd.devel` (`org.freedesktop.ratbag_devel1`) instead
    /// of production ratbagd. Useful when developing against a locally
    /// built libratbag tree.
    #[arg(long)]
    pub devel: bool,

    /// Path to the persistent rule file. Defaults to
    /// `$XDG_CONFIG_HOME/gamerat/rules.toml`.
    #[arg(long, value_name = "PATH")]
    pub rules: Option<PathBuf>,

    /// Verbosity: `-v` for debug, `-vv` for trace. Overridden by
    /// `RUST_LOG` if set.
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Which focus backend to use. `auto` tries wlr and falls back to
    /// synthetic-only if the compositor doesn't support it. `wlr`
    /// requires it (errors out if unavailable). `synthetic` skips real
    /// detection entirely — useful for tests + headless CI.
    ///
    /// In all modes the synthetic backend is also attached, so
    /// `gameratctl focus simulate` keeps working alongside the real
    /// backend.
    #[arg(long, value_enum, default_value_t = BackendMode::Auto)]
    pub backend: BackendMode,
}

/// Which focus backend the daemon should run.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum BackendMode {
    /// Try `wlr`, fall back to synthetic-only if unavailable.
    #[default]
    Auto,
    /// Require `wlr-foreign-toplevel-management-unstable-v1`.
    Wlr,
    /// Synthetic only.
    Synthetic,
}

/// Daemon entry point. Returns when SIGINT or SIGTERM is received.
pub async fn run(args: Args) -> Result<()> {
    init_tracing(args.verbose);

    let rules_path = match args.rules {
        Some(p) => p,
        None => paths::default_rules_path()?,
    };
    let rules = RuleStore::load_or_create(rules_path).context("loading persisted rules")?;
    let rules = std::sync::Arc::new(RwLock::new(rules));

    let ratbag_service = if args.devel {
        RatbagService::Devel
    } else {
        RatbagService::Production
    };
    let ratbag = gamerat_ratbag::Client::connect_to(ratbag_service.clone())
        .await
        .with_context(|| format!("connecting to ratbagd (`{}`)", ratbag_service.bus_name()))?;
    info!(service = %ratbag_service.bus_name(), "ratbagd connected");

    let (injector, synth_backend) = SyntheticBackend::new();
    let status = std::sync::Arc::new(RwLock::new(DaemonStatus::default()));
    let handle = AppHandle::new(rules.clone(), ratbag.clone(), injector, status.clone());

    let focus_stream = build_focus_stream(args.backend, synth_backend)?;

    let conn = zbus::connection::Builder::session()
        .context("opening session bus")?
        .name(gamerat_proto::BUS_NAME)
        .context("requesting bus name")?
        .serve_at(
            gamerat_proto::OBJECT_PATH,
            GameRatService::new(handle.clone()),
        )
        .context("registering interface")?
        .build()
        .await
        .with_context(|| format!("claiming `{}` on the session bus", gamerat_proto::BUS_NAME))?;
    info!(bus_name = gamerat_proto::BUS_NAME, "interface registered");

    let dispatch_handle = handle.clone();
    let dispatch_conn = conn.clone();
    let dispatch_task = tokio::spawn(async move {
        if let Err(e) = run_dispatch(dispatch_handle, focus_stream, dispatch_conn).await {
            warn!(error = ?e, "dispatch loop terminated with error");
        }
    });

    // Wait for shutdown signals.
    let mut sigterm = signal(SignalKind::terminate()).context("installing SIGTERM handler")?;
    let mut sigint = signal(SignalKind::interrupt()).context("installing SIGINT handler")?;
    tokio::select! {
        _ = sigterm.recv() => info!("SIGTERM, shutting down"),
        _ = sigint.recv() => info!("SIGINT, shutting down"),
    }

    dispatch_task.abort();
    Ok(())
}

/// Assemble the focus stream the dispatch loop will consume. The
/// synthetic backend is always included so the daemon's
/// `SimulateFocus` D-Bus method remains usable (for tests + ad-hoc CLI
/// invocation) even when a real backend is also active.
fn build_focus_stream(mode: BackendMode, synth: SyntheticBackend) -> Result<FocusStream> {
    let synth_stream = synth.into_stream();

    let real: Option<FocusStream> = match mode {
        BackendMode::Synthetic => None,
        BackendMode::Wlr => match WlrForeignToplevelBackend::try_connect() {
            Ok(b) => {
                info!("focus backend: wlr-foreign-toplevel-management-unstable-v1");
                Some(b.into_stream())
            }
            Err(e) => {
                return Err(anyhow::Error::new(e)
                    .context("--backend wlr requested but compositor support missing"));
            }
        },
        BackendMode::Auto => match WlrForeignToplevelBackend::try_connect() {
            Ok(b) => {
                info!("focus backend: wlr (auto-detected)");
                Some(b.into_stream())
            }
            Err(e) => {
                info!(reason = %e, "focus backend: synthetic only (wlr unavailable)");
                None
            }
        },
    };

    Ok(match real {
        Some(real) => Box::pin(futures::stream::select(synth_stream, real)),
        None => synth_stream,
    })
}

fn init_tracing(verbose: u8) {
    let default_level = match verbose {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("gamerat={default_level},warn")));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .init();
}
