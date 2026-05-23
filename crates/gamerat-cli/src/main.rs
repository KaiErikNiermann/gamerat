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
    ButtonAction, GameRatProxy, GameratProfile, ProfileLed, button_action_kind, button_special,
    compat_warning, game_category, led_color_depth, led_mode, macro_event_kind,
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

    /// Read / write per-LED hardware state (color + mode + brightness).
    #[command(subcommand)]
    Led(LedCmd),

    /// Toggle the focus-driven autoswitch behaviour.
    #[command(subcommand)]
    Autoswitch(AutoswitchCmd),

    /// Stream `FocusChanged` + `ProfileSwitched` signals until Ctrl-C.
    Watch,

    /// Recover from a stuck-key macro: rebinds the button to a release-
    /// only macro, prompts you to press it once, then auto-disables
    /// the binding after 5 seconds.
    Panic {
        /// 0-based device index (see `gameratctl device list`).
        #[arg(long, default_value_t = 0)]
        device: usize,
        /// Button index whose macro should be defused.
        button: u32,
    },

    /// Diagnose the soft-macro / uinput pipeline (status + setup
    /// guidance for the `input` group + `/dev/uinput` permissions).
    #[command(subcommand)]
    SoftInput(SoftInputCmd),
}

#[derive(Debug, Subcommand)]
enum SoftInputCmd {
    /// Print the runtime state of every piece of the soft-input
    /// pipeline (master flag, `/dev/uinput`, evdev nodes, group
    /// membership) and, if anything is broken, the exact commands
    /// to fix it.
    Status,
    /// Same as `status` but framed as "here's what you need to do
    /// to make this work". Convenient first-run entry point.
    Setup,
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
enum LedCmd {
    /// List every LED on a device's profile + its current state.
    List {
        /// 0-based index into `gameratctl device list`.
        #[arg(long, default_value_t = 0)]
        device: usize,
        /// Hardware profile index. Defaults to the currently active
        /// profile.
        #[arg(long)]
        profile: Option<u32>,
    },
    /// Write one LED's mode + color + brightness.
    Set {
        /// 0-based device index.
        #[arg(long, default_value_t = 0)]
        device: usize,
        /// Hardware profile index. Defaults to the currently active
        /// profile.
        #[arg(long)]
        profile: Option<u32>,
        /// LED index on the device (0-based).
        #[arg(long)]
        led: u32,
        /// LED operating mode.
        #[arg(long, value_enum, default_value_t = LedModeArg::Solid)]
        mode: LedModeArg,
        /// `#rrggbb` (case-insensitive). Required for `solid` and
        /// `breathing`; ignored for `off` and `cycle`. Defaults to
        /// `#ffffff` when omitted in a color-driven mode.
        #[arg(long)]
        color: Option<String>,
        /// 0..=255. Defaults to 255 (max).
        #[arg(long, default_value_t = 255)]
        brightness: u32,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum LedModeArg {
    Off,
    /// Solid fixed color.
    Solid,
    /// Auto-cycle through colors (firmware does the rainbow).
    Cycle,
    Breathing,
}

impl LedModeArg {
    const fn as_wire(self) -> u32 {
        match self {
            Self::Off => led_mode::OFF,
            Self::Solid => led_mode::ON,
            Self::Cycle => led_mode::CYCLE,
            Self::Breathing => led_mode::BREATHING,
        }
    }
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

    /// Read / write per-LED state declared in a saved profile.
    #[command(subcommand)]
    Led(ProfileLedCmd),

    /// Read / write software-side soft-macros (currently: sticky
    /// toggles) attached to a saved profile.
    #[command(subcommand)]
    SoftMacro(ProfileSoftMacroCmd),
}

#[derive(Debug, Subcommand)]
enum ProfileSoftMacroCmd {
    /// Show the soft-macros declared inside a saved profile.
    List {
        /// Profile id.
        id: String,
    },
    /// Add or replace a sticky-toggle soft-macro for `button` in the
    /// given profile. The toggled keycodes are taken from `--keys`.
    Set {
        /// Profile id.
        id: String,
        /// Hardware button index (matches `gameratctl button list`).
        button: u32,
        /// Comma-separated Linux keycodes the toggle emits.
        #[arg(long, value_delimiter = ',', num_args = 1..)]
        keys: Vec<u32>,
    },
    /// Drop the soft-macro entry for `button` from the profile.
    Clear {
        /// Profile id.
        id: String,
        /// Hardware button index.
        button: u32,
    },
}

#[derive(Debug, Subcommand)]
enum ProfileLedCmd {
    /// Show the per-LED state declared in a saved profile.
    List { id: String },
    /// Set one LED's state inside a saved profile. The change is
    /// written via `SetProfile`; run `profile apply` afterwards to
    /// push it to hardware if the profile is currently materialised.
    Set {
        id: String,
        #[arg(long)]
        led: u32,
        #[arg(long, value_enum, default_value_t = LedModeArg::Solid)]
        mode: LedModeArg,
        #[arg(long)]
        color: Option<String>,
        #[arg(long, default_value_t = 255)]
        brightness: u32,
    },
    /// Remove an LED entry from a saved profile (LED state reverts to
    /// hardware default on next materialise).
    Delete {
        id: String,
        #[arg(long)]
        led: u32,
    },
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
        Command::Led(cmd) => cmd_led(&proxy, cmd).await,
        Command::Autoswitch(cmd) => cmd_autoswitch(&proxy, cmd).await,
        Command::Watch => cmd_watch(&proxy).await,
        Command::Panic { device, button } => cmd_panic(&proxy, device, button).await,
        Command::SoftInput(cmd) => cmd_soft_input(&proxy, cmd).await,
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

async fn cmd_led(proxy: &GameRatProxy<'_>, cmd: LedCmd) -> Result<()> {
    match cmd {
        LedCmd::List { device, profile } => cmd_led_list(proxy, device, profile).await,
        LedCmd::Set {
            device,
            profile,
            led,
            mode,
            color,
            brightness,
        } => cmd_led_set(proxy, device, profile, led, mode, color, brightness).await,
    }
}

async fn cmd_led_list(
    proxy: &GameRatProxy<'_>,
    device_index: usize,
    profile: Option<u32>,
) -> Result<()> {
    let device_path = pick_device_path(proxy, device_index).await?;
    let profile_slot = profile.unwrap_or(u32::MAX);
    let leds = proxy
        .list_leds(device_path, profile_slot)
        .await
        .context("ListLeds failed")?;
    if leds.is_empty() {
        println!("(no LEDs reported — device driver may not expose any)");
        return Ok(());
    }
    println!(
        "{:<5} {:<10} {:<10} {:<5} {:<22} depth",
        "idx", "mode", "color", "brt", "supported_modes"
    );
    for l in &leds {
        println!(
            "{:<5} {:<10} {:<10} {:<5} {:<22} {}",
            l.index,
            led_mode_name(l.mode),
            format_color(l.color),
            l.brightness,
            format_supported_modes(&l.supported_modes),
            led_color_depth_name(l.color_depth),
        );
    }
    Ok(())
}

async fn cmd_led_set(
    proxy: &GameRatProxy<'_>,
    device_index: usize,
    profile: Option<u32>,
    led_index: u32,
    mode: LedModeArg,
    color: Option<String>,
    brightness: u32,
) -> Result<()> {
    let device_path = pick_device_path(proxy, device_index).await?;
    let profile_slot = profile.unwrap_or(u32::MAX);
    let payload = build_profile_led(led_index, mode, color.as_deref(), brightness)?;
    proxy
        .set_led(device_path, profile_slot, led_index, payload)
        .await
        .context("SetLed failed")?;
    println!("ok");
    Ok(())
}

/// Build a `ProfileLed` from CLI flags. Color is required (and parsed)
/// only when the mode actually consumes a color — `off` / `cycle`
/// silently accept any color and write `(255, 255, 255)` as a stable
/// default so the wire payload is uniform.
fn build_profile_led(
    index: u32,
    mode: LedModeArg,
    color_hex: Option<&str>,
    brightness: u32,
) -> Result<ProfileLed> {
    let color = match mode {
        LedModeArg::Solid | LedModeArg::Breathing => {
            parse_hex_color(color_hex.unwrap_or("#ffffff"))?
        }
        LedModeArg::Off | LedModeArg::Cycle => {
            // Color is irrelevant in these modes; persist whatever the
            // user passed (or pure white) so re-reading the field
            // round-trips cleanly.
            color_hex
                .map(parse_hex_color)
                .transpose()?
                .unwrap_or((255, 255, 255))
        }
    };
    let brightness = brightness.min(255);
    Ok(ProfileLed {
        index,
        mode: mode.as_wire(),
        color,
        brightness,
    })
}

/// Parse `#rrggbb` (case-insensitive, leading `#` optional) into a
/// `(r, g, b)` tuple of u32s in 0..=255.
fn parse_hex_color(s: &str) -> Result<(u32, u32, u32)> {
    let trimmed = s.trim().trim_start_matches('#');
    if trimmed.len() != 6 {
        anyhow::bail!("expected 6-digit hex color (e.g. `#ff3344`), got `{s}`");
    }
    let parse = |slice: &str| {
        u32::from_str_radix(slice, 16).with_context(|| format!("invalid hex component in `{s}`"))
    };
    Ok((
        parse(&trimmed[0..2])?,
        parse(&trimmed[2..4])?,
        parse(&trimmed[4..6])?,
    ))
}

fn format_color((r, g, b): (u32, u32, u32)) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

const fn led_mode_name(mode: u32) -> &'static str {
    match mode {
        x if x == led_mode::OFF => "off",
        x if x == led_mode::ON => "solid",
        x if x == led_mode::CYCLE => "cycle",
        x if x == led_mode::BREATHING => "breathing",
        _ => "unknown",
    }
}

const fn led_color_depth_name(depth: u32) -> &'static str {
    match depth {
        x if x == led_color_depth::MONOCHROME => "monochrome",
        x if x == led_color_depth::RGB_888 => "rgb-888",
        x if x == led_color_depth::RGB_111 => "rgb-111",
        _ => "unknown",
    }
}

fn format_supported_modes(modes: &[u32]) -> String {
    if modes.is_empty() {
        return "(none)".to_owned();
    }
    modes
        .iter()
        .map(|m| led_mode_name(*m))
        .collect::<Vec<_>>()
        .join(",")
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
                // CLI's `profile add` never sets bindings, LED state,
                // or soft-macros at creation time — use `profile
                // button set` / `profile led set` / `soft-macro set`
                // to populate them afterwards (or edit via the GUI).
                buttons: Vec::new(),
                leds: Vec::new(),
                soft_macros: Vec::new(),
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
        ProfileCmd::Led(cmd) => cmd_profile_led(proxy, cmd).await,
        ProfileCmd::SoftMacro(cmd) => cmd_profile_soft_macro(proxy, cmd).await,
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

async fn cmd_profile_soft_macro(proxy: &GameRatProxy<'_>, cmd: ProfileSoftMacroCmd) -> Result<()> {
    use gamerat_proto::{SoftMacro, soft_macro_kind};

    match cmd {
        ProfileSoftMacroCmd::List { id } => {
            let profile = proxy.get_profile(&id).await.context("GetProfile failed")?;
            if profile.soft_macros.is_empty() {
                println!("(no soft-macros declared in profile `{id}`)");
                return Ok(());
            }
            println!("{:<5} {:<14} {:<10} keys", "btn", "kind", "trampoline",);
            for m in &profile.soft_macros {
                let kind = if m.kind == soft_macro_kind::STICKY_TOGGLE {
                    "sticky-toggle"
                } else {
                    "disabled"
                };
                let trampoline = if m.trampoline_keycode == 0 {
                    "—".to_owned()
                } else {
                    format!("KEY_{}", m.trampoline_keycode)
                };
                let keys = m
                    .keys
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                println!("{:<5} {kind:<14} {trampoline:<10} {keys}", m.button_index);
            }
            Ok(())
        }
        ProfileSoftMacroCmd::Set { id, button, keys } => {
            if keys.is_empty() {
                anyhow::bail!("`--keys` must list at least one keycode");
            }
            let mut profile = proxy.get_profile(&id).await.context("GetProfile failed")?;
            let entry = SoftMacro {
                button_index: button,
                kind: soft_macro_kind::STICKY_TOGGLE,
                // Daemon allocates a trampoline on first apply; the
                // CLI never needs to choose one.
                trampoline_keycode: 0,
                keys,
            };
            if let Some(existing) = profile
                .soft_macros
                .iter_mut()
                .find(|m| m.button_index == button)
            {
                *existing = entry;
            } else {
                profile.soft_macros.push(entry);
                profile.soft_macros.sort_by_key(|m| m.button_index);
            }
            proxy
                .set_profile(profile)
                .await
                .context("SetProfile failed")?;
            println!("ok");
            Ok(())
        }
        ProfileSoftMacroCmd::Clear { id, button } => {
            let mut profile = proxy.get_profile(&id).await.context("GetProfile failed")?;
            let before = profile.soft_macros.len();
            profile.soft_macros.retain(|m| m.button_index != button);
            if profile.soft_macros.len() == before {
                println!("(no soft-macro for button {button} in profile `{id}`)");
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

async fn cmd_profile_led(proxy: &GameRatProxy<'_>, cmd: ProfileLedCmd) -> Result<()> {
    match cmd {
        ProfileLedCmd::List { id } => {
            let profile = proxy.get_profile(&id).await.context("GetProfile failed")?;
            if profile.leds.is_empty() {
                println!("(no per-LED state declared in profile `{id}`)");
                return Ok(());
            }
            println!("{:<5} {:<10} {:<10} brt", "idx", "mode", "color");
            for l in &profile.leds {
                println!(
                    "L{:<4} {:<10} {:<10} {}",
                    l.index,
                    led_mode_name(l.mode),
                    format_color(l.color),
                    l.brightness,
                );
            }
            Ok(())
        }
        ProfileLedCmd::Set {
            id,
            led,
            mode,
            color,
            brightness,
        } => {
            let mut profile = proxy.get_profile(&id).await.context("GetProfile failed")?;
            let payload = build_profile_led(led, mode, color.as_deref(), brightness)?;
            if let Some(existing) = profile.leds.iter_mut().find(|l| l.index == led) {
                *existing = payload;
            } else {
                profile.leds.push(payload);
                profile.leds.sort_by_key(|l| l.index);
            }
            proxy
                .set_profile(profile)
                .await
                .context("SetProfile failed")?;
            println!("ok");
            Ok(())
        }
        ProfileLedCmd::Delete { id, led } => {
            let mut profile = proxy.get_profile(&id).await.context("GetProfile failed")?;
            let before = profile.leds.len();
            profile.leds.retain(|l| l.index != led);
            if profile.leds.len() == before {
                println!("(no LED entry for index {led} in profile `{id}`)");
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

/// Probe the host for everything the soft-input pipeline needs, and
/// print a per-resource check + remediation steps. Does not modify any
/// system state — purely informational; the user runs the suggested
/// `sudo` themselves.
///
/// Designed to make the most common failure mode ("user isn't in the
/// `input` group yet") obvious and self-fixable without spelunking
/// through journalctl.
// Each check + remediation is a small step; the dispatch lives in one
// place so the user gets a single coherent report. Splitting per-row
// would scatter the formatting across helpers without making the code
// easier to follow.
#[allow(clippy::too_many_lines)]
async fn cmd_soft_input(proxy: &GameRatProxy<'_>, cmd: SoftInputCmd) -> Result<()> {
    let is_setup = matches!(cmd, SoftInputCmd::Setup);
    if is_setup {
        println!("soft-input setup\n");
    } else {
        println!("soft-input status\n");
    }

    // 1. Master opt-in.
    let enabled = proxy
        .software_macros_enabled()
        .await
        .context("reading SoftwareMacrosEnabled")?;
    println!(
        "[{}] master flag (settings → Enable soft-macros)",
        if enabled { "ok" } else { ".." }
    );
    if !enabled {
        println!(
            "    -> off; soft-macros are disabled. Enable in the GUI's Settings,\n\
             \x20      or `busctl --user set-property org.appulsauce.GameRat1 \\\n\
             \x20        /org/appulsauce/GameRat1 org.appulsauce.GameRat1 \\\n\
             \x20        SoftwareMacrosEnabled b true`, then restart the daemon.",
        );
    }

    // 2. Daemon-reported aggregate state (what the GUI pill shows).
    let state = proxy
        .soft_input_state()
        .await
        .context("reading SoftInputState")?;
    println!("[{}] daemon-reported state: {state}", state_tag(&state));

    // 3. /dev/uinput — needed for synthetic key emission.
    let uinput_writable = check_writable("/dev/uinput");
    println!(
        "[{}] /dev/uinput writable by you",
        if uinput_writable { "ok" } else { "fail" }
    );
    if !uinput_writable {
        println!(
            "    -> the kernel uinput device is missing or unwritable. The\n\
             \x20      packaged install ships a udev rule that grants the\n\
             \x20      `input` group access; if you're running from source,\n\
             \x20      copy it manually:\n\
             \x20        sudo cp packaging/udev/60-gamerat-uinput.rules /etc/udev/rules.d/\n\
             \x20        sudo udevadm control --reload-rules\n\
             \x20        sudo udevadm trigger --subsystem-match=misc",
        );
    }

    // 4. /dev/input/event* readability (read=evdev access).
    let evdev_readable = check_any_evdev_readable();
    println!(
        "[{}] /dev/input/event* readable by you",
        if evdev_readable { "ok" } else { "fail" }
    );

    // 5. `input` group membership — two checks because "in /etc/group"
    //    and "in this process's getgroups()" can disagree. On KDE Plasma
    //    + systemd-user, the user manager started at boot keeps its
    //    original supplementary-group set; usermod + a new login
    //    session don't refresh it. Distinguishing the two failure modes
    //    is the difference between "run usermod" and "reboot".
    let in_input_static = user_in_group_static("input");
    let in_input_process = user_in_group_runtime("input");
    let group_row_label = match (in_input_static, in_input_process) {
        (true, true) => ("ok", None),
        (true, false) => (
            "stale",
            Some(
                "    -> `/etc/group` says you're a member of `input`, but the\n\
                 \x20      current login session can't see it. Your `systemd --user`\n\
                 \x20      manager (started at boot) is holding the old group set\n\
                 \x20      and feeding it to the whole desktop. Fix with either:\n\
                 \x20        reboot                                  (cleanest), or\n\
                 \x20        loginctl terminate-user $USER           (run from a\n\
                 \x20          fresh TTY: Ctrl+Alt+F2, then log in again on\n\
                 \x20          Ctrl+Alt+F1 — closes every desktop process).",
            ),
        ),
        (false, _) => (
            "fail",
            Some(
                "    -> add yourself with:\n\
                 \x20        sudo usermod -aG input $USER\n\
                 \x20      then log out + back in. If a plain relogin doesn't\n\
                 \x20      pick the new group up (KDE Plasma + systemd-user\n\
                 \x20      caches the gid set on its manager), reboot.",
            ),
        ),
    };
    println!(
        "[{}] you're a member of the `input` group",
        group_row_label.0
    );
    if let Some(hint) = group_row_label.1 {
        println!("{hint}");
    }
    let in_input_group = in_input_static && in_input_process;

    // 6. Bottom-line summary.
    println!();
    if enabled && state == "active" && uinput_writable && evdev_readable && in_input_group {
        println!("soft-input is fully online; soft-toggles will fire as configured.",);
    } else if !enabled {
        println!(
            "soft-input is intentionally disabled. No fix needed unless you want \
             to enable it.",
        );
    } else {
        println!(
            "soft-input is INERT — bindings configured as soft-toggles behave \
             as inactive trampoline keycodes. Fix the failing rows above, then \
             restart the daemon.",
        );
    }
    Ok(())
}

fn state_tag(state: &str) -> &'static str {
    match state {
        "active" => "ok",
        "disabled" => "..",
        _ => "warn",
    }
}

/// Quick probe: does opening `path` for write succeed without
/// allocating anything? Used to detect `/dev/uinput` permission
/// without pulling in the full evdev crate from the CLI.
fn check_writable(path: &str) -> bool {
    std::fs::OpenOptions::new().write(true).open(path).is_ok()
}

/// Scan `/dev/input/event*` and try opening one for read. We don't
/// care which node — just whether the kernel + udev let us into the
/// input subsystem at all (which is essentially "are we in the
/// `input` group, or has an ACL granted access").
fn check_any_evdev_readable() -> bool {
    let Ok(entries) = std::fs::read_dir("/dev/input") else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("event") {
            continue;
        }
        if std::fs::OpenOptions::new()
            .read(true)
            .open(entry.path())
            .is_ok()
        {
            return true;
        }
    }
    false
}

/// Runtime membership probe via `getgroups(2)` — what THIS process
/// can currently see. Doesn't reflect `/etc/group` edits made after
/// the parent process tree was launched (see [`user_in_group_static`]
/// for that view).
fn user_in_group_runtime(group_name: &str) -> bool {
    let Some(target_gid) = lookup_gid(group_name) else {
        return false;
    };
    // SAFETY: getgroups(0, NULL) returns the required buffer size; we
    // size + reread on the second call.
    let n = unsafe { libc_getgroups(0, std::ptr::null_mut()) };
    let Ok(n) = usize::try_from(n) else {
        return false;
    };
    if n == 0 {
        return false;
    }
    let mut buf: Vec<u32> = vec![0; n];
    let Ok(size) = std::ffi::c_int::try_from(n) else {
        return false;
    };
    let got = unsafe { libc_getgroups(size, buf.as_mut_ptr()) };
    let Ok(got) = usize::try_from(got) else {
        return false;
    };
    buf.truncate(got);
    buf.contains(&target_gid)
}

/// Authoritative membership probe via `getgrouplist(3)` — the set
/// of groups the kernel WOULD give a fresh login of this user. Reads
/// `/etc/group`, `/etc/passwd`, NSS, etc., so it reflects post-boot
/// `usermod` edits even when the running session manager hasn't
/// picked them up yet.
///
/// `getgrouplist` requires the username + primary gid as inputs;
/// we read those from `getpwuid` for the current effective uid.
fn user_in_group_static(group_name: &str) -> bool {
    let Some(target_gid) = lookup_gid(group_name) else {
        return false;
    };
    // SAFETY: getpwuid returns a pointer to a static buffer; we only
    // deref to read the name + primary gid before invalidation.
    let (user_cstr, primary_gid) = unsafe {
        let uid = libc_geteuid();
        let pw = libc_getpwuid(uid);
        if pw.is_null() {
            return false;
        }
        // Copy the name into our own buffer immediately — getgrouplist
        // and any other libc call may invalidate the static buffer.
        let name_ptr = (*pw).pw_name;
        if name_ptr.is_null() {
            return false;
        }
        let mut len = 0usize;
        while *name_ptr.add(len) != 0 {
            len += 1;
        }
        let slice = std::slice::from_raw_parts(name_ptr.cast::<u8>(), len);
        let Ok(cstr) = std::ffi::CString::new(slice) else {
            return false;
        };
        (cstr, (*pw).pw_gid)
    };

    // Two-call pattern: first to find the required size, second to
    // actually fill the buffer.
    let mut ngroups: std::ffi::c_int = 0;
    // SAFETY: passing a null buffer with ngroups=0 is the documented
    // size-query form. The return value is -1 (buffer too small) and
    // *ngroups is updated to the required size.
    unsafe {
        libc_getgrouplist(
            user_cstr.as_ptr(),
            primary_gid,
            std::ptr::null_mut(),
            &raw mut ngroups,
        );
    }
    let Ok(n) = usize::try_from(ngroups) else {
        return false;
    };
    if n == 0 {
        return false;
    }
    let mut buf: Vec<u32> = vec![0; n];
    let got = unsafe {
        libc_getgrouplist(
            user_cstr.as_ptr(),
            primary_gid,
            buf.as_mut_ptr(),
            &raw mut ngroups,
        )
    };
    if got < 0 {
        return false;
    }
    let Ok(filled) = usize::try_from(ngroups) else {
        return false;
    };
    buf.truncate(filled);
    buf.contains(&target_gid)
}

fn lookup_gid(group_name: &str) -> Option<u32> {
    let cname = std::ffi::CString::new(group_name).ok()?;
    // SAFETY: getgrnam returns a pointer to a static buffer; we only
    // read gr_gid before any subsequent libc call.
    unsafe {
        let entry = libc_getgrnam(cname.as_ptr());
        if entry.is_null() {
            return None;
        }
        Some((*entry).gr_gid)
    }
}

// Minimal libc shims so we don't add a libc dep just for a handful of
// calls. All the underlying functions are POSIX standard.
#[repr(C)]
struct LibcGroup {
    _gr_name: *const std::ffi::c_char,
    _gr_passwd: *const std::ffi::c_char,
    gr_gid: u32,
    _gr_mem: *const *const std::ffi::c_char,
}

#[repr(C)]
struct LibcPasswd {
    pw_name: *const std::ffi::c_char,
    _pw_passwd: *const std::ffi::c_char,
    _pw_uid: u32,
    pw_gid: u32,
    _pw_gecos: *const std::ffi::c_char,
    _pw_dir: *const std::ffi::c_char,
    _pw_shell: *const std::ffi::c_char,
}

unsafe extern "C" {
    #[link_name = "getgrnam"]
    fn libc_getgrnam(name: *const std::ffi::c_char) -> *const LibcGroup;
    #[link_name = "getgroups"]
    fn libc_getgroups(size: std::ffi::c_int, list: *mut u32) -> std::ffi::c_int;
    #[link_name = "getpwuid"]
    fn libc_getpwuid(uid: u32) -> *const LibcPasswd;
    #[link_name = "geteuid"]
    fn libc_geteuid() -> u32;
    #[link_name = "getgrouplist"]
    fn libc_getgrouplist(
        user: *const std::ffi::c_char,
        group: u32,
        groups: *mut u32,
        ngroups: *mut std::ffi::c_int,
    ) -> std::ffi::c_int;
}

async fn cmd_panic(proxy: &GameRatProxy<'_>, device_index: usize, button: u32) -> Result<()> {
    use gamerat_proto::PanicHatchSettledArgs;

    let device_path = pick_device_path(proxy, device_index).await?;

    // Subscribe before firing the method so a fast-path settle
    // (timeout race, immediate cancel) isn't missed.
    let mut settled = proxy
        .receive_panic_hatch_settled()
        .await
        .context("subscribing to PanicHatchSettled")?;

    let (released_keys, awaiting_press) = proxy
        .panic_hatch(device_path.clone(), button)
        .await
        .context("PanicHatch failed")?;

    if !awaiting_press {
        if released_keys.is_empty() {
            println!("binding cleared (no stuck keys detected)");
        } else {
            println!(
                "cleared stuck keys: {}",
                released_keys
                    .iter()
                    .map(|k| format!("KEY_{k}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        return Ok(());
    }

    println!(
        "stuck keys detected ({}). press button {button} now to release; auto-disabling in 5s",
        released_keys
            .iter()
            .map(|k| format!("KEY_{k}"))
            .collect::<Vec<_>>()
            .join(", "),
    );

    loop {
        tokio::select! {
            Some(signal) = settled.next() => {
                let args: PanicHatchSettledArgs<'_> =
                    signal.args().context("decoding PanicHatchSettled")?;
                if args.device.as_str() != device_path.as_str() || args.button != button {
                    continue;
                }
                match args.outcome {
                    "timeout_disabled" => println!("timeout fired, binding disabled"),
                    "superseded" => println!("binding was changed elsewhere; left alone"),
                    "cancelled" => println!("cancelled"),
                    other => println!("settled: {other}"),
                }
                return Ok(());
            }
            _ = tokio::signal::ctrl_c() => {
                eprintln!("\n^C, cancelling panic-hatch");
                proxy
                    .cancel_panic_hatch(device_path.clone(), button)
                    .await
                    .context("CancelPanicHatch failed")?;
                // Fall through to the next loop iteration so the
                // "cancelled" settle signal is what prints the closer.
            }
            else => return Ok(()),
        }
    }
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
