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
use clap::{Parser, Subcommand, ValueEnum};
use futures::StreamExt as _;
use gamerat_proto::{
    ButtonAction, GameRatProxy, GameratProfile, button_action_kind, button_special, compat_warning,
    game_category, macro_event_kind,
};

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

    /// Manage user-defined software profiles.
    #[command(subcommand)]
    Profile(ProfileCmd),

    /// Read / write per-button hardware bindings via ratbagd.
    #[command(subcommand)]
    Button(ButtonCmd),

    /// Toggle the focus-driven autoswitch behaviour.
    #[command(subcommand)]
    Autoswitch(AutoswitchCmd),

    /// Stream `FocusChanged` + `ProfileSwitched` signals until Ctrl-C.
    Watch,
}

#[derive(Debug, Subcommand)]
enum ButtonCmd {
    /// List every button on a device's profile + its current binding.
    List {
        /// 0-based index into `gameratctl device list`. Defaults to the
        /// first device.
        #[arg(long, default_value_t = 0)]
        device: usize,
        /// Hardware profile index. Defaults to the currently active
        /// profile.
        #[arg(long)]
        profile: Option<u32>,
    },
    /// Write a binding to one button.
    Set {
        /// 0-based device index.
        #[arg(long, default_value_t = 0)]
        device: usize,
        /// Hardware profile index. Defaults to the currently active
        /// profile.
        #[arg(long)]
        profile: Option<u32>,
        /// Button index on the device (0-based).
        button: u32,
        /// Action to write.
        #[command(subcommand)]
        action: ActionArg,
    },
}

#[derive(Debug, Subcommand)]
enum ActionArg {
    /// Disable the button.
    None,
    /// Map to another hardware mouse button index.
    Mouse {
        /// Target mouse button (0 = left, 1 = right, 2 = middle, ...).
        target: u32,
    },
    /// Bind a special action — see the `button_special` constants
    /// (e.g. `wheel-down`, `resolution-cycle-up`).
    Special {
        /// Kebab-case name (or numeric value) of the special action.
        name: String,
    },
    /// Bind a single Linux keycode (see `linux/input-event-codes.h`).
    Key {
        /// Numeric keycode.
        code: u32,
    },
}

#[derive(Debug, Subcommand)]
enum AutoswitchCmd {
    /// Print the current state.
    Status,
    /// Enable rule-driven profile switching on focus events.
    On,
    /// Stop switching profiles automatically; focus events still emit
    /// `FocusChanged` so the GUI updates, but no rule action fires.
    Off,
    /// Flip the current value.
    Toggle,
}

#[derive(Debug, Subcommand)]
enum RuleCmd {
    /// Add or replace a rule for an `app_id` glob.
    Add {
        /// Glob to match against the focused window's `app_id`.
        glob: String,
        /// Profile id (see `gameratctl profile list`) to apply when
        /// this rule matches. The daemon accepts unknown ids
        /// (so rules can be authored before profiles); CLI surfaces
        /// a warning before submit.
        #[arg(short, long, value_name = "ID")]
        profile_id: String,
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
    /// Show the hardware slot map for a device — which gamerat
    /// profile (if any) currently occupies each slot, plus the
    /// active and desktop markers.
    Slots {
        /// 0-based device index. Defaults to the first device.
        #[arg(long, default_value_t = 0)]
        device: usize,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CategoryArg {
    /// Reusable across games (e.g. "fps-low-dpi", "mmo-multi-button").
    Agnostic,
    /// Tied to one specific game (e.g. "cs2", "mw3").
    Specific,
}

impl CategoryArg {
    const fn as_wire(self) -> &'static str {
        match self {
            Self::Agnostic => game_category::AGNOSTIC,
            Self::Specific => game_category::SPECIFIC,
        }
    }
}

#[derive(Debug, Subcommand)]
enum ProfileCmd {
    /// List every user-defined profile.
    List,
    /// Print one profile in full detail.
    Show {
        /// Profile id (e.g. `fps-low-dpi`).
        id: String,
    },
    /// Add or replace a profile.
    Add {
        /// Profile id — must be kebab-case (lowercase / digits / -/_).
        #[arg(long)]
        id: String,
        /// Human-readable name.
        #[arg(long)]
        name: String,
        /// Reusable layer or tied to one game.
        #[arg(long, value_enum, default_value_t = CategoryArg::Agnostic)]
        category: CategoryArg,
        /// Comma-separated DPI stages (e.g. `--dpi 400,800,1600`).
        #[arg(long, value_delimiter = ',', num_args = 1..)]
        dpi: Vec<u32>,
        /// Default-active DPI stage (zero-based index into `--dpi`).
        #[arg(long, default_value_t = 0)]
        active: u32,
        /// Free-text description.
        #[arg(long, default_value = "")]
        description: String,
        /// Agnostic-profile id this profile inherits from
        /// (future equivalence-dedup hint; harmless if empty).
        #[arg(long, value_name = "ID", default_value = "")]
        inherits_from: String,
    },
    /// Delete a profile by id.
    Delete { id: String },
    /// Force a saved profile onto the device immediately. Bypasses
    /// focus rules and the autoswitch flag — useful for testing
    /// from the terminal or as a fallback when the GUI is unhappy.
    Apply {
        /// Profile id to apply.
        id: String,
    },
    /// Edit / list per-button bindings stored inside a saved
    /// profile (NOT the hardware-direct surface — see `button set`
    /// for that).
    #[command(subcommand)]
    Button(ProfileButtonCmd),
}

#[derive(Debug, Subcommand)]
enum ProfileButtonCmd {
    /// Show the per-button bindings declared in a saved profile.
    List {
        /// Profile id.
        id: String,
    },
    /// Set one binding inside a saved profile. The change is
    /// written to disk via `SetProfile`; if the profile is currently
    /// materialised, run `profile apply` to push it to hardware.
    Set {
        /// Profile id.
        id: String,
        /// Hardware button index (0-based).
        button: u32,
        /// Action to bind — reuses the same `<action>` subcommand
        /// shape as the hardware-direct `button set`.
        #[command(subcommand)]
        action: ActionArg,
    },
    /// Remove a binding from a saved profile (button reverts to
    /// "no override" — applies hardware default on next materialise).
    Delete {
        /// Profile id.
        id: String,
        /// Hardware button index.
        button: u32,
    },
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

    // Probe ratbagd's APIVersion once and warn if it's outside the
    // window we've validated against. Never blocks — older or newer
    // ratbagd may still work for the subset of methods we exercise.
    if let Ok(Some(compat)) = gamerat_ratbag::probe_compat().await
        && let Some(msg) = compat_warning(compat)
    {
        eprintln!("warning: {msg}");
    }

    match cli.command {
        Command::Status => cmd_status(&proxy).await,
        Command::Rule(RuleCmd::Add { glob, profile_id }) => {
            // Surface a warning if the user references a profile that
            // doesn't exist yet — daemon accepts it, but flag the
            // typo case at the source.
            match proxy.list_profiles().await {
                Ok(profiles) if !profiles.iter().any(|p| p.id == profile_id) => {
                    eprintln!(
                        "warning: profile `{profile_id}` not in store yet (rule will be inert \
                         until created)"
                    );
                }
                _ => {}
            }
            proxy
                .set_rule(&glob, &profile_id)
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
        Command::Device(DeviceCmd::Slots { device }) => cmd_device_slots(&proxy, device).await,
        Command::Games(GamesCmd::List { launcher }) => cmd_games_list(&proxy, launcher).await,
        Command::Profile(cmd) => cmd_profile(&proxy, cmd).await,
        Command::Button(cmd) => cmd_button(&proxy, cmd).await,
        Command::Autoswitch(cmd) => cmd_autoswitch(&proxy, cmd).await,
        Command::Watch => cmd_watch(&proxy).await,
    }
}

async fn cmd_button(proxy: &GameRatProxy<'_>, cmd: ButtonCmd) -> Result<()> {
    match cmd {
        ButtonCmd::List { device, profile } => cmd_button_list(proxy, device, profile).await,
        ButtonCmd::Set {
            device,
            profile,
            button,
            action,
        } => cmd_button_set(proxy, device, profile, button, action).await,
    }
}

async fn cmd_button_list(
    proxy: &GameRatProxy<'_>,
    device_index: usize,
    profile: Option<u32>,
) -> Result<()> {
    let device_path = pick_device_path(proxy, device_index).await?;
    let profile_slot = profile.unwrap_or(u32::MAX);
    let buttons = proxy
        .list_buttons(device_path, profile_slot)
        .await
        .context("ListButtons failed")?;
    if buttons.is_empty() {
        println!("(no buttons reported)");
        return Ok(());
    }
    println!(
        "{:<6} {:<22} {:<14} action",
        "idx", "action_kind", "supports"
    );
    for b in &buttons {
        println!(
            "{:<6} {:<22} {:<14} {}",
            b.index,
            kind_name(b.action.kind),
            format_supported(&b.supported_action_types),
            format_action(&b.action),
        );
    }
    Ok(())
}

async fn cmd_button_set(
    proxy: &GameRatProxy<'_>,
    device_index: usize,
    profile: Option<u32>,
    button_index: u32,
    action_arg: ActionArg,
) -> Result<()> {
    let device_path = pick_device_path(proxy, device_index).await?;
    let action = action_arg_to_action(action_arg)?;
    let profile_slot = profile.unwrap_or(u32::MAX);
    proxy
        .set_button(device_path, profile_slot, button_index, action)
        .await
        .context("SetButton failed")?;
    println!("ok");
    Ok(())
}

async fn pick_device_path(
    proxy: &GameRatProxy<'_>,
    index: usize,
) -> Result<zbus::zvariant::OwnedObjectPath> {
    let devices = proxy.list_devices().await.context("ListDevices failed")?;
    devices
        .into_iter()
        .nth(index)
        .map(|d| d.object_path)
        .ok_or_else(|| anyhow::anyhow!("no device at index {index} (run `gameratctl device list`)"))
}

fn action_arg_to_action(arg: ActionArg) -> Result<ButtonAction> {
    Ok(match arg {
        ActionArg::None => ButtonAction::none(),
        ActionArg::Mouse { target } => ButtonAction::mouse(target),
        ActionArg::Special { name } => {
            let v = parse_special(&name).ok_or_else(|| {
                anyhow::anyhow!(
                    "unknown special action `{name}` — try one of: doubleclick, wheel-left, \
                     wheel-right, wheel-up, wheel-down, ratchet-mode-switch, \
                     resolution-cycle-up, resolution-cycle-down, resolution-up, \
                     resolution-down, resolution-alternate, resolution-default, \
                     profile-cycle-up, profile-cycle-down, profile-up, profile-down, \
                     second-mode, battery-level"
                )
            })?;
            ButtonAction::special(v)
        }
        ActionArg::Key { code } => ButtonAction::key(code),
    })
}

fn parse_special(name: &str) -> Option<u32> {
    // Allow a literal numeric value as an escape hatch — useful when
    // ratbagd grows a new special before we add it to the table.
    if let Ok(n) = name.parse::<u32>() {
        return Some(n);
    }
    match name.to_lowercase().as_str() {
        "unknown" => Some(button_special::UNKNOWN),
        "doubleclick" => Some(button_special::DOUBLECLICK),
        "wheel-left" => Some(button_special::WHEEL_LEFT),
        "wheel-right" => Some(button_special::WHEEL_RIGHT),
        "wheel-up" => Some(button_special::WHEEL_UP),
        "wheel-down" => Some(button_special::WHEEL_DOWN),
        "ratchet-mode-switch" => Some(button_special::RATCHET_MODE_SWITCH),
        "resolution-cycle-up" => Some(button_special::RESOLUTION_CYCLE_UP),
        "resolution-cycle-down" => Some(button_special::RESOLUTION_CYCLE_DOWN),
        "resolution-up" => Some(button_special::RESOLUTION_UP),
        "resolution-down" => Some(button_special::RESOLUTION_DOWN),
        "resolution-alternate" => Some(button_special::RESOLUTION_ALTERNATE),
        "resolution-default" => Some(button_special::RESOLUTION_DEFAULT),
        "profile-cycle-up" => Some(button_special::PROFILE_CYCLE_UP),
        "profile-cycle-down" => Some(button_special::PROFILE_CYCLE_DOWN),
        "profile-up" => Some(button_special::PROFILE_UP),
        "profile-down" => Some(button_special::PROFILE_DOWN),
        "second-mode" => Some(button_special::SECOND_MODE),
        "battery-level" => Some(button_special::BATTERY_LEVEL),
        _ => None,
    }
}

const fn kind_name(kind: u32) -> &'static str {
    match kind {
        button_action_kind::NONE => "NONE",
        button_action_kind::MOUSE => "MOUSE",
        button_action_kind::SPECIAL => "SPECIAL",
        button_action_kind::KEY => "KEY",
        button_action_kind::MACRO => "MACRO",
        _ => "UNKNOWN",
    }
}

fn format_supported(types: &[u32]) -> String {
    let mut out = String::new();
    for (i, t) in types.iter().enumerate() {
        if i > 0 {
            out.push('+');
        }
        out.push_str(match *t {
            button_action_kind::NONE => "none",
            button_action_kind::MOUSE => "btn",
            button_action_kind::SPECIAL => "spec",
            button_action_kind::KEY => "key",
            button_action_kind::MACRO => "mac",
            _ => "?",
        });
    }
    out
}

fn format_action(a: &ButtonAction) -> String {
    match a.kind {
        button_action_kind::NONE => "disabled".to_owned(),
        button_action_kind::MOUSE => format!("mouse({})", a.value),
        button_action_kind::SPECIAL => format!("special({:#x})", a.value),
        button_action_kind::KEY => format!("key({})", a.value),
        button_action_kind::MACRO => {
            let steps: Vec<String> = a
                .macro_steps
                .iter()
                .map(|s| {
                    let prefix = match s.kind {
                        macro_event_kind::KEY_PRESS => "p",
                        macro_event_kind::KEY_RELEASE => "r",
                        macro_event_kind::WAIT => "w",
                        _ => "?",
                    };
                    format!("{prefix}:{}", s.value)
                })
                .collect();
            format!("macro[{}]", steps.join(", "))
        }
        _ => format!("?({}, {})", a.kind, a.value),
    }
}

async fn cmd_autoswitch(proxy: &GameRatProxy<'_>, cmd: AutoswitchCmd) -> Result<()> {
    let current = proxy
        .auto_switch_enabled()
        .await
        .context("reading AutoSwitchEnabled")?;
    match cmd {
        AutoswitchCmd::Status => {
            println!("autoswitch: {}", if current { "on" } else { "off" });
        }
        AutoswitchCmd::On | AutoswitchCmd::Off | AutoswitchCmd::Toggle => {
            let next = match cmd {
                AutoswitchCmd::On => true,
                AutoswitchCmd::Off => false,
                AutoswitchCmd::Toggle => !current,
                AutoswitchCmd::Status => unreachable!(),
            };
            proxy
                .set_auto_switch_enabled(next)
                .await
                .context("writing AutoSwitchEnabled")?;
            println!("autoswitch: {}", if next { "on" } else { "off" });
        }
    }
    Ok(())
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
            "{:width$}  → {}",
            rule.app_id_glob,
            rule.profile_id,
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
            "{}\n    name        {}\n    model       {}\n    profile     {} of {}\n    dpi stages  up to {}\n",
            d.object_path.as_str(),
            d.name,
            d.model,
            d.active_profile,
            d.profile_count,
            d.max_dpi_stages,
        );
    }
    Ok(())
}

async fn cmd_profile(proxy: &GameRatProxy<'_>, cmd: ProfileCmd) -> Result<()> {
    match cmd {
        ProfileCmd::List => cmd_profile_list(proxy).await,
        ProfileCmd::Show { id } => cmd_profile_show(proxy, &id).await,
        ProfileCmd::Add {
            id,
            name,
            category,
            dpi,
            active,
            description,
            inherits_from,
        } => {
            let profile = GameratProfile {
                id,
                name,
                description,
                category: category.as_wire().to_owned(),
                inherits_from,
                dpi,
                active_dpi_stage: active,
                created_unix: 0, // 0 lets the daemon stamp it.
                // CLI's `profile add` never sets bindings at
                // creation time — use `profile button set` to
                // populate them afterwards (or edit via the GUI).
                buttons: Vec::new(),
            };
            proxy
                .set_profile(profile)
                .await
                .context("SetProfile failed")?;
            println!("ok");
            Ok(())
        }
        ProfileCmd::Delete { id } => {
            proxy
                .delete_profile(&id)
                .await
                .context("DeleteProfile failed")?;
            println!("ok");
            Ok(())
        }
        ProfileCmd::Apply { id } => {
            proxy
                .apply_profile(&id)
                .await
                .context("ApplyProfile failed")?;
            println!("ok");
            Ok(())
        }
        ProfileCmd::Button(cmd) => cmd_profile_button(proxy, cmd).await,
    }
}

async fn cmd_profile_button(proxy: &GameRatProxy<'_>, cmd: ProfileButtonCmd) -> Result<()> {
    match cmd {
        ProfileButtonCmd::List { id } => {
            let profile = proxy.get_profile(&id).await.context("GetProfile failed")?;
            if profile.buttons.is_empty() {
                println!("(no per-button bindings declared in profile `{id}`)");
                return Ok(());
            }
            for b in &profile.buttons {
                println!("B{:<3}  {}", b.index, format_action(&b.action));
            }
            Ok(())
        }
        ProfileButtonCmd::Set { id, button, action } => {
            let mut profile = proxy.get_profile(&id).await.context("GetProfile failed")?;
            let new_action = action_arg_to_action(action)?;
            // Replace any existing binding for this index, otherwise
            // append. Stable order keeps profiles.toml diffs minimal.
            if let Some(existing) = profile.buttons.iter_mut().find(|b| b.index == button) {
                existing.action = new_action;
            } else {
                profile.buttons.push(gamerat_proto::ProfileButton {
                    index: button,
                    action: new_action,
                });
                profile.buttons.sort_by_key(|b| b.index);
            }
            proxy
                .set_profile(profile)
                .await
                .context("SetProfile failed")?;
            println!("ok");
            Ok(())
        }
        ProfileButtonCmd::Delete { id, button } => {
            let mut profile = proxy.get_profile(&id).await.context("GetProfile failed")?;
            let before = profile.buttons.len();
            profile.buttons.retain(|b| b.index != button);
            if profile.buttons.len() == before {
                println!("(no binding for button {button} in profile `{id}`)");
                return Ok(());
            }
            proxy
                .set_profile(profile)
                .await
                .context("SetProfile failed")?;
            println!("ok");
            Ok(())
        }
    }
}

async fn cmd_device_slots(proxy: &GameRatProxy<'_>, device_index: usize) -> Result<()> {
    let device_path = pick_device_path(proxy, device_index).await?;
    let slots = proxy
        .get_slot_map(device_path)
        .await
        .context("GetSlotMap failed")?;
    if slots.is_empty() {
        println!("(no slots reported — allocator may not be initialised yet)");
        return Ok(());
    }
    println!(
        "{:<5} {:<20} {:<8} {:<7}",
        "slot", "profile_id", "active?", "role"
    );
    for s in &slots {
        let role = if s.is_desktop { "desktop" } else { "managed" };
        let id = if s.profile_id.is_empty() {
            "(empty)"
        } else {
            &s.profile_id
        };
        let active = if s.is_active { "*" } else { " " };
        println!("{:<5} {:<20} {:<8} {:<7}", s.index, id, active, role);
    }
    Ok(())
}

async fn cmd_profile_list(proxy: &GameRatProxy<'_>) -> Result<()> {
    let profiles = proxy.list_profiles().await.context("ListProfiles failed")?;
    if profiles.is_empty() {
        println!("(no profiles)");
        return Ok(());
    }
    let widest_id = profiles.iter().map(|p| p.id.len()).max().unwrap_or(0);
    let widest_name = profiles
        .iter()
        .map(|p| p.name.chars().count())
        .max()
        .unwrap_or(0);
    for p in &profiles {
        let dpi = p
            .dpi
            .iter()
            .enumerate()
            .map(|(i, v)| {
                if u32::try_from(i).is_ok_and(|i| i == p.active_dpi_stage) {
                    format!("*{v}")
                } else {
                    v.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        println!(
            "{:id$}  {:8}  {:name$}  dpi={}",
            p.id,
            p.category,
            p.name,
            dpi,
            id = widest_id,
            name = widest_name,
        );
    }
    println!("\n{} profile(s)", profiles.len());
    Ok(())
}

async fn cmd_profile_show(proxy: &GameRatProxy<'_>, id: &str) -> Result<()> {
    let p = proxy
        .get_profile(id)
        .await
        .with_context(|| format!("GetProfile {id}"))?;
    println!("id            {}", p.id);
    println!("name          {}", p.name);
    println!("category      {}", p.category);
    if !p.inherits_from.is_empty() {
        println!("inherits      {}", p.inherits_from);
    }
    if !p.description.is_empty() {
        println!("description   {}", p.description);
    }
    println!(
        "dpi stages    {}",
        p.dpi
            .iter()
            .enumerate()
            .map(
                |(i, v)| if u32::try_from(i).is_ok_and(|i| i == p.active_dpi_stage) {
                    format!("*{v}")
                } else {
                    v.to_string()
                }
            )
            .collect::<Vec<_>>()
            .join(", "),
    );
    println!("created       {}", p.created_unix);
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
