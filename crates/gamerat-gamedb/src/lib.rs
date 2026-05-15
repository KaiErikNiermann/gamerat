//! Game library discovery across Linux launchers.
//!
//! Maps installed games on the host to stable identifiers and
//! launch-time signatures (executable paths, Wine prefixes, Steam
//! `AppID`s) so the daemon can match focused processes back to the game
//! the user means.
//!
//! Planned scanners:
//!
//! - Steam — `~/.steam/steam/steamapps/libraryfolders.vdf` walk +
//!   `appmanifest_*.acf` parsing.
//! - Lutris — `~/.local/share/lutris/games/` + the sqlite db.
//! - Heroic — `~/.config/heroic/store/library.json`.
//! - Generic — `.desktop` file scrape under `$XDG_DATA_DIRS`.
//!
//! Scaffolding only.
