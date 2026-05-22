//! KDE focus-bridge management.
//!
//! On Plasma 6 the daemon can't observe window focus directly — KWin
//! doesn't expose `wlr-foreign-toplevel-management` to unprivileged
//! clients. Focus only reaches the daemon through the `gamerat-focus`
//! KWin script (shipped in `data/kwin-script/`), which runs inside the
//! compositor and calls `IngestKwinFocus`. But KWin only loads enabled
//! scripts at session start, and a mid-session KWin restart (a Plasma
//! update, a crash) silently drops them — leaving auto-switch dead with
//! no signal to the user.
//!
//! This module lets the daemon take care of that itself: detect a KDE
//! session, make sure the script is installed in a KWin-scanned dir,
//! enabled in `kwinrc`, and loaded into the running compositor — then
//! report the resulting [`FocusBridgeState`] so the GUI can surface an
//! actionable error if anything's still wrong. The same probe / repair
//! is exposed over D-Bus (`CheckFocusBridge` / `EnsureKwinFocusBridge`)
//! so the GUI's "Repair" button drives identical logic.
//!
//! Everything here is user-scope (no root): the script lives under
//! `~/.local/share/kwin/scripts/` and the load happens over the session
//! bus via `org.kde.KWin`'s Scripting interface.

// "KWin" is a proper noun that recurs in nearly every doc line here;
// backticking each occurrence hurts readability more than it helps.
#![allow(clippy::doc_markdown)]

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use directories::BaseDirs;
use tracing::{debug, info, warn};
use zbus::fdo::DBusProxy;
use zbus::names::BusName;
use zbus::{Connection, Proxy};

/// KPackage plugin id of the focus bridge — the directory name under
/// `kwin/scripts/`, the `kwinrc` `[Plugins]` key prefix, and the
/// `loadScript` plugin name all key off this.
const PLUGIN_ID: &str = "gamerat-focus";

const KWIN_SERVICE: &str = "org.kde.KWin";
const SCRIPTING_PATH: &str = "/Scripting";
const SCRIPTING_IFACE: &str = "org.kde.kwin.Scripting";

/// Health of the KDE focus bridge. Mirrors the wire-stable strings in
/// [`gamerat_proto::focus_bridge`]; [`Self::as_wire`] round-trips to
/// the value the D-Bus methods return.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FocusBridgeState {
    /// KDE session + script loaded — focus events flow.
    Active,
    /// KDE session but the script isn't loaded — auto-switch is inert.
    NotLoaded,
    /// Not a KWin session (wlr / X11 / synthetic) — bridge irrelevant.
    NotApplicable,
    /// Couldn't probe KWin's Scripting interface.
    Unknown,
}

impl FocusBridgeState {
    /// Wire-stable string for the `CheckFocusBridge` /
    /// `EnsureKwinFocusBridge` D-Bus return.
    #[must_use]
    pub const fn as_wire(self) -> &'static str {
        use gamerat_proto::focus_bridge as fb;
        match self {
            Self::Active => fb::ACTIVE,
            Self::NotLoaded => fb::NOT_LOADED,
            Self::NotApplicable => fb::NOT_APPLICABLE,
            Self::Unknown => fb::UNKNOWN,
        }
    }
}

/// Probe the bridge state without changing anything. One `org.kde.KWin`
/// round-trip on KDE; a single `NameHasOwner` elsewhere.
pub async fn check(conn: &Connection) -> FocusBridgeState {
    if !is_kde_session(conn).await {
        return FocusBridgeState::NotApplicable;
    }
    match is_script_loaded(conn).await {
        Ok(true) => FocusBridgeState::Active,
        Ok(false) => FocusBridgeState::NotLoaded,
        Err(e) => {
            warn!(error = ?e, "KWin isScriptLoaded probe failed");
            FocusBridgeState::Unknown
        }
    }
}

/// Idempotently install + enable + load the focus bridge, returning the
/// resulting state. Safe to call repeatedly (startup + every GUI
/// "Repair" click); each step is a no-op when already satisfied.
pub async fn ensure(conn: &Connection) -> FocusBridgeState {
    if !is_kde_session(conn).await {
        debug!("not a KWin session; focus bridge not applicable");
        return FocusBridgeState::NotApplicable;
    }

    let main_js = match locate_or_install() {
        Ok(p) => p,
        Err(e) => {
            warn!(error = ?e, "couldn't locate or install the gamerat-focus KWin script");
            return FocusBridgeState::Unknown;
        }
    };

    // Persist enablement so KWin auto-loads it on the next login. The
    // live load below is what makes it work *this* session.
    enable_in_kwinrc();

    match is_script_loaded(conn).await {
        Ok(true) => return FocusBridgeState::Active,
        Ok(false) => {}
        Err(e) => warn!(error = ?e, "isScriptLoaded probe failed; attempting load anyway"),
    }

    if let Err(e) = load_script(conn, &main_js).await {
        warn!(error = ?e, "KWin loadScript/start failed");
        return FocusBridgeState::NotLoaded;
    }

    // Confirm the load actually took.
    match is_script_loaded(conn).await {
        Ok(true) => {
            info!("gamerat-focus KWin script loaded — focus bridge active");
            FocusBridgeState::Active
        }
        Ok(false) => FocusBridgeState::NotLoaded,
        Err(_) => FocusBridgeState::Unknown,
    }
}

/// Is `org.kde.KWin` on the session bus? The cheapest reliable "are we
/// on a KWin session?" check — independent of `$XDG_CURRENT_DESKTOP`
/// spoofing and of which focus backend the daemon happens to run.
async fn is_kde_session(conn: &Connection) -> bool {
    let Ok(dbus) = DBusProxy::new(conn).await else {
        return false;
    };
    let Ok(name) = BusName::try_from(KWIN_SERVICE) else {
        return false;
    };
    dbus.name_has_owner(name).await.unwrap_or(false)
}

async fn scripting_proxy(conn: &Connection) -> zbus::Result<Proxy<'static>> {
    Proxy::new(conn, KWIN_SERVICE, SCRIPTING_PATH, SCRIPTING_IFACE).await
}

async fn is_script_loaded(conn: &Connection) -> zbus::Result<bool> {
    scripting_proxy(conn)
        .await?
        .call("isScriptLoaded", &(PLUGIN_ID,))
        .await
}

/// `loadScript(path, pluginId)` + `start()`. KWin's `reconfigure`
/// doesn't hot-load newly-enabled scripts on Plasma 6.6+, so the
/// explicit load is the only thing that brings the bridge up mid-session.
async fn load_script(conn: &Connection, main_js: &Path) -> zbus::Result<()> {
    let proxy = scripting_proxy(conn).await?;
    let path = main_js.to_string_lossy();
    // Returns the script id; we don't need it.
    let _id: i32 = proxy
        .call("loadScript", &(path.as_ref(), PLUGIN_ID))
        .await?;
    proxy.call::<_, _, ()>("start", &()).await?;
    Ok(())
}

/// Find the `main.js` to load: prefer a copy already in a KWin-scanned
/// directory (system or user), otherwise install one from a source
/// bundle into the user scripts dir and return that.
fn locate_or_install() -> Result<PathBuf> {
    for dir in kwin_scanned_dirs() {
        let main_js = dir.join(PLUGIN_ID).join("contents/code/main.js");
        if main_js.is_file() {
            debug!(path = %main_js.display(), "found existing KWin script");
            return Ok(main_js);
        }
    }

    let src = resolve_source_bundle()
        .context("no gamerat-focus script bundle found (looked in /usr/share/kwin/scripts, /usr/share/gamerat/kwin-script, the dev data dir, and $GAMERAT_KWIN_SCRIPT_DIR)")?;
    let dest = user_scripts_dir()?.join(PLUGIN_ID);
    copy_dir_all(&src, &dest)
        .with_context(|| format!("installing KWin script into {}", dest.display()))?;
    info!(dest = %dest.display(), "installed gamerat-focus KWin script");
    Ok(dest.join("contents/code/main.js"))
}

/// Directories KWin scans for `KWin/Script` packages, most-specific
/// first (user overrides system for a shared plugin id). Mirrors KWin's
/// own discovery: the user data dir plus every `$XDG_DATA_DIRS` entry —
/// so a tarball install under `/usr/local/share` is found alongside a
/// distro package under `/usr/share`.
fn kwin_scanned_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(user) = user_scripts_dir() {
        dirs.push(user);
    }
    let data_dirs =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".to_owned());
    for base in data_dirs.split(':').filter(|s| !s.is_empty()) {
        dirs.push(Path::new(base).join("kwin").join("scripts"));
    }
    dirs
}

fn user_scripts_dir() -> Result<PathBuf> {
    let base = BaseDirs::new().context("could not determine $HOME for the KWin scripts dir")?;
    Ok(base.data_dir().join("kwin").join("scripts"))
}

/// Locate a source bundle to install from when no KWin-scanned copy
/// exists yet. Used in dev (the repo `data/` tree) and as a fallback if
/// a package shipped the bundle to gamerat's private data dir rather
/// than a KWin scripts dir.
fn resolve_source_bundle() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(over) = std::env::var_os("GAMERAT_KWIN_SCRIPT_DIR") {
        candidates.push(PathBuf::from(over));
    }
    candidates.push(PathBuf::from(
        "/usr/share/gamerat/kwin-script/gamerat-focus",
    ));
    // Dev tree: <crate>/../../data/kwin-script/gamerat-focus, baked at
    // compile time. Only valid on the build host — packaged installs
    // hit a KWin-scanned dir first and never reach here.
    candidates.push(PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/kwin-script/gamerat-focus"
    )));
    candidates
        .into_iter()
        .find(|p| p.join("metadata.json").is_file())
}

/// Recursively copy a directory tree (the bundle is tiny: a
/// `metadata.json` plus `contents/code/main.js`).
fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Flip `kwinrc [Plugins] gamerat-focusEnabled=true` via `kwriteconfig`
/// (shipped by kconfig, so virtually always present on a KDE session).
/// Best-effort: a missing tool only costs cross-login persistence — the
/// live `loadScript` still brings the bridge up for this session.
fn enable_in_kwinrc() {
    for tool in ["kwriteconfig6", "kwriteconfig5"] {
        match std::process::Command::new(tool)
            .args([
                "--file",
                "kwinrc",
                "--group",
                "Plugins",
                "--key",
                "gamerat-focusEnabled",
                "true",
            ])
            .status()
        {
            Ok(status) if status.success() => {
                debug!(tool, "enabled gamerat-focus in kwinrc");
                return;
            }
            Ok(status) => warn!(tool, code = ?status.code(), "kwriteconfig exited non-zero"),
            Err(_) => {} // tool not found — try the next, then warn.
        }
    }
    warn!(
        "kwriteconfig6/5 not found — gamerat-focus loaded for this session only \
         (it won't persist across logins; enable it in \
         System Settings → KWin Scripts to make it stick)"
    );
}
