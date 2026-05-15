# gamerat — documentation

Project documentation lives here. As of scaffolding there are no
documents yet; planned entries:

- `architecture.md` — daemon ↔ ratbagd ↔ focus backends ↔ GUI overview
- `dbus-interface.md` — wire-level reference for `org.appulsauce.GameRat1`
- `profile-model.md` — what a "software-abstracted hardware profile" means
- `focus-backends.md` — how the X11 / Wayland / KWin backends differ
- `contributing.md` — local dev workflow, lint expectations, commit style

Build user-facing docs (when they exist) with whatever the maintainer of
the doc set prefers — likely `mdbook` for the prose portion and
zbus-generated XML → markdown for the interface reference.
