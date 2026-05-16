//! `gameratctl` — scriptable client for the gamerat daemon.
//!
//! Every subcommand is a thin wrapper around one method or signal on
//! the daemon's `org.appulsauce.GameRat1` interface. The CLI is the
//! *only* client until the GUI lands, so it doubles as the
//! integration-test driver.

// CLI output is the whole point of this crate, so the project-wide
// print_stdout / print_stderr warnings would just clutter the file.
// stdout: command results. stderr: progress / status messages.
#![allow(clippy::print_stdout, clippy::print_stderr)]
// We run on a current-thread tokio runtime; Send-bound futures aren't
// required and StdoutLock / D-Bus proxy futures aren't Send.
#![allow(clippy::future_not_send)]

use std::path::PathBuf;

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

    /// Discover installed games.
    #[command(subcommand)]
    Games(GamesCmd),

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
    /// Stream-write incoming `FocusChanged` signals to a TOML fixture
    /// file, suitable for replay via `gamerat-daemon --replay-fixture`.
    /// Records until Ctrl-C; the file is flushed after every event so
    /// partial captures are usable.
    Record {
        /// Output path. Defaults to stdout.
        #[arg(short, long, value_name = "PATH")]
        output: Option<PathBuf>,
        /// Free-form description written to the fixture's `[meta]`
        /// block. Useful for distinguishing recordings later.
        #[arg(long, default_value = "")]
        description: String,
    },
}

#[derive(Debug, Subcommand)]
enum DeviceCmd {
    /// Enumerate ratbagd-managed devices.
    List,
}

#[derive(Debug, Subcommand)]
enum GamesCmd {
    /// Print every game the daemon's launcher scanners discovered at
    /// startup (Steam / Lutris / Heroic). Filter with `--launcher`.
    List {
        /// Show only games from this launcher
        /// (`steam` | `lutris` | `heroic` | `other`).
        #[arg(long, value_name = "TAG")]
        launcher: Option<String>,
    },
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
        Command::Focus(FocusCmd::Record {
            output,
            description,
        }) => cmd_focus_record(&proxy, output, description).await,
        Command::Device(DeviceCmd::List) => cmd_device_list(&proxy).await,
        Command::Games(GamesCmd::List { launcher }) => cmd_games_list(&proxy, launcher).await,
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

async fn cmd_games_list(proxy: &GameRatProxy<'_>, launcher: Option<String>) -> Result<()> {
    let mut games = proxy.list_games().await.context("ListGames failed")?;
    if let Some(filter) = launcher.as_deref() {
        games.retain(|g| g.launcher == filter);
    }
    if games.is_empty() {
        println!("(no games)");
        return Ok(());
    }
    games.sort_by(|a, b| a.launcher.cmp(&b.launcher).then(a.name.cmp(&b.name)));

    let widest_name = games
        .iter()
        .map(|g| g.name.chars().count())
        .max()
        .unwrap_or(0);
    let widest_hint = games
        .iter()
        .map(|g| g.app_id_hint.chars().count())
        .max()
        .unwrap_or(0);

    for g in &games {
        let hint = if g.app_id_hint.is_empty() {
            "—"
        } else {
            &g.app_id_hint
        };
        println!(
            "{:7}  {:name$}  {:hint$}  {}",
            g.launcher,
            g.name,
            hint,
            g.id,
            name = widest_name,
            hint = widest_hint,
        );
    }
    println!("\n{} game(s)", games.len());
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

/// Escape a string into a TOML basic-string literal. Hand-rolled to
/// avoid pulling toml in as a CLI dep just for this — the recorder
/// only ever emits a fixed handful of fields.
fn toml_basic_string(s: &str) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                // String::write_fmt never fails — discard the Result.
                let _ = write!(out, "\\u{:04X}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

async fn cmd_focus_record(
    proxy: &GameRatProxy<'_>,
    output: Option<PathBuf>,
    description: String,
) -> Result<()> {
    use std::io::Write as _;
    use std::time::Instant;

    use gamerat_proto::FocusChangedArgs;

    let mut writer: Box<dyn std::io::Write> = if let Some(path) = output.as_deref() {
        eprintln!("recording to {}", path.display());
        let file = std::fs::File::create(path)
            .with_context(|| format!("creating output {}", path.display()))?;
        Box::new(std::io::BufWriter::new(file))
    } else {
        eprintln!("recording to stdout");
        Box::new(std::io::stdout().lock())
    };

    writeln!(writer, "# gamerat focus fixture")?;
    writeln!(
        writer,
        "# Recorded by `gameratctl focus record`. Replay with:"
    )?;
    writeln!(writer, "#   gamerat-daemon --replay-fixture <this-file>")?;
    writeln!(writer)?;
    writeln!(writer, "[meta]")?;
    writeln!(writer, "description = {}", toml_basic_string(&description))?;
    // Per-event source is preserved below; leave meta.source empty so
    // the replayer doesn't paper over a mixed-source recording.
    writeln!(writer, "source      = \"\"")?;
    writeln!(writer)?;
    writer.flush()?;

    let mut focus = proxy
        .receive_focus_changed()
        .await
        .context("subscribing to FocusChanged")?;
    let mut last: Option<Instant> = None;
    let mut count: u64 = 0;

    eprintln!("recording focus events (Ctrl-C to stop)…");

    loop {
        tokio::select! {
            Some(signal) = focus.next() => {
                let args: FocusChangedArgs<'_> =
                    signal.args().context("decoding FocusChanged")?;
                let now = Instant::now();
                let delay_ms: u64 = last.map_or(0, |t| {
                    u64::try_from(now.duration_since(t).as_millis()).unwrap_or(u64::MAX)
                });

                writeln!(writer, "[[event]]")?;
                writeln!(writer, "delay_ms = {delay_ms}")?;
                writeln!(writer, "app_id   = {}", toml_basic_string(args.app_id))?;
                writeln!(writer, "title    = {}", toml_basic_string(args.title))?;
                writeln!(writer, "source   = {}", toml_basic_string(args.source))?;
                writeln!(writer)?;
                writer.flush()?;

                last = Some(now);
                count += 1;
                eprintln!("  {count:>4}: {} ({})", args.app_id, args.source);
            }
            _ = tokio::signal::ctrl_c() => {
                eprintln!("\ndone — wrote {count} event(s)");
                writer.flush()?;
                return Ok(());
            }
            else => {
                writer.flush()?;
                return Ok(());
            }
        }
    }
}
