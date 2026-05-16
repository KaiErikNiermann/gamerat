//! `gameratctl` — scriptable client for the gamerat daemon.
//!
//! Every subcommand is a thin wrapper around one method or signal on
//! the daemon's `org.appulsauce.GameRat1` interface. The CLI is the
//! *only* client until the GUI lands, so it doubles as the
//! integration-test driver.

// CLI output is the whole point of this crate, so the project-wide
// print_stdout warning would just clutter the file.
#![allow(clippy::print_stdout)]

use anyhow::{Context as _, Result};
use clap::{Parser, Subcommand};
use futures::StreamExt as _;
use gamerat_proto::GameRatProxy;

#[derive(Debug, Parser)]
#[command(name = "gameratctl", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print a one-shot status snapshot from the daemon.
    Status,

    /// Manage focus rules.
    #[command(subcommand)]
    Rule(RuleCmd),

    /// Synthesize / inspect focus events.
    #[command(subcommand)]
    Focus(FocusCmd),

    /// Query ratbagd via the daemon.
    #[command(subcommand)]
    Device(DeviceCmd),

    /// Stream `FocusChanged` + `ProfileSwitched` signals until Ctrl-C.
    Watch,
}

#[derive(Debug, Subcommand)]
enum RuleCmd {
    /// Add or replace a rule for an `app_id` glob.
    Add {
        /// Glob to match against the focused window's `app_id`.
        glob: String,
        /// Zero-based profile index to switch the device to.
        #[arg(short, long)]
        profile: u32,
    },
    /// List all rules in the daemon's store.
    List,
    /// Delete a rule by its exact glob string.
    Delete {
        /// The glob to remove. Must match exactly (use `rule list` to
        /// see registered globs).
        glob: String,
    },
}

#[derive(Debug, Subcommand)]
enum FocusCmd {
    /// Inject a synthetic focus event into the daemon.
    Simulate {
        /// App identifier the rule matcher will see.
        app_id: String,
        /// Optional window title.
        #[arg(long, default_value = "")]
        title: String,
    },
}

#[derive(Debug, Subcommand)]
enum DeviceCmd {
    /// Enumerate ratbagd-managed devices.
    List,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(dispatch(cli))
}

async fn dispatch(cli: Cli) -> Result<()> {
    let conn = zbus::Connection::session()
        .await
        .context("opening session bus")?;
    let proxy = GameRatProxy::new(&conn)
        .await
        .context("connecting to daemon (is gamerat-daemon running?)")?;

    match cli.command {
        Command::Status => cmd_status(&proxy).await,
        Command::Rule(RuleCmd::Add { glob, profile }) => {
            proxy
                .set_rule(&glob, profile)
                .await
                .context("SetRule failed")?;
            println!("ok");
            Ok(())
        }
        Command::Rule(RuleCmd::List) => cmd_rule_list(&proxy).await,
        Command::Rule(RuleCmd::Delete { glob }) => {
            proxy
                .delete_rule(&glob)
                .await
                .context("DeleteRule failed")?;
            println!("ok");
            Ok(())
        }
        Command::Focus(FocusCmd::Simulate { app_id, title }) => {
            proxy
                .simulate_focus(&app_id, &title)
                .await
                .context("SimulateFocus failed")?;
            println!("ok");
            Ok(())
        }
        Command::Device(DeviceCmd::List) => cmd_device_list(&proxy).await,
        Command::Watch => cmd_watch(&proxy).await,
    }
}

async fn cmd_status(proxy: &GameRatProxy<'_>) -> Result<()> {
    let status = proxy.status().await.context("Status failed")?;
    let version = proxy.version().await.unwrap_or_else(|_| "?".to_owned());
    println!("daemon       {version}");
    println!("focused      {}", show_or_dash(&status.focused_app_id));
    println!("last switch  {}", show_or_dash(&status.last_switch_reason));
    println!("rules loaded {}", status.rules_loaded);
    Ok(())
}

async fn cmd_rule_list(proxy: &GameRatProxy<'_>) -> Result<()> {
    let rules = proxy.list_rules().await.context("ListRules failed")?;
    if rules.is_empty() {
        println!("(no rules)");
        return Ok(());
    }
    let widest = rules.iter().map(|r| r.app_id_glob.len()).max().unwrap_or(0);
    for rule in rules {
        println!(
            "{:width$}  profile {}",
            rule.app_id_glob,
            rule.profile_index,
            width = widest
        );
    }
    Ok(())
}

async fn cmd_device_list(proxy: &GameRatProxy<'_>) -> Result<()> {
    let devices = proxy.list_devices().await.context("ListDevices failed")?;
    if devices.is_empty() {
        println!("(no devices)");
        return Ok(());
    }
    for d in devices {
        println!(
            "{}\n    name    {}\n    model   {}\n    profile {} of {}\n",
            d.object_path.as_str(),
            d.name,
            d.model,
            d.active_profile,
            d.profile_count,
        );
    }
    Ok(())
}

async fn cmd_watch(proxy: &GameRatProxy<'_>) -> Result<()> {
    use gamerat_proto::{FocusChangedArgs, ProfileSwitchedArgs};

    let mut focus = proxy
        .receive_focus_changed()
        .await
        .context("subscribing to FocusChanged")?;
    let mut switched = proxy
        .receive_profile_switched()
        .await
        .context("subscribing to ProfileSwitched")?;

    println!("watching (Ctrl-C to exit)");
    loop {
        tokio::select! {
            Some(signal) = focus.next() => {
                let args: FocusChangedArgs<'_> =
                    signal.args().context("decoding FocusChanged")?;
                println!(
                    "focus    {} \"{}\" (src={})",
                    args.app_id, args.title, args.source,
                );
            }
            Some(signal) = switched.next() => {
                let args: ProfileSwitchedArgs<'_> =
                    signal.args().context("decoding ProfileSwitched")?;
                println!(
                    "switch   {} : {} -> {} ({})",
                    args.device.as_str(),
                    args.from_profile,
                    args.to_profile,
                    args.reason,
                );
            }
            _ = tokio::signal::ctrl_c() => {
                println!("\nbye");
                return Ok(());
            }
            else => return Ok(()),
        }
    }
}

const fn show_or_dash(s: &str) -> &str {
    if s.is_empty() { "—" } else { s }
}
