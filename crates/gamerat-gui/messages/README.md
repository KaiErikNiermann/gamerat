# Translations

GUI strings live here as one JSON catalog per locale, in the [inlang
message format](https://inlang.com/m/reesgh7l/plugin-inlang-messageFormat).
[Paraglide JS](https://inlang.com/m/gerre34r/library-inlang-paraglideJs)
compiles them into type-safe `m.*()` functions under
`src/lib/paraglide/` (generated, gitignored).

- **`en.json`** — the source of truth (English). Edit this when adding or
  changing UI text.
- **`de.json`**, … — translations, normally produced via Crowdin.

## Workflow

1. Add/΄change a key in `en.json` (kebab-ish `area_thing` naming; use
   `{param}` placeholders for interpolation — keep the same param names in
   every locale).
2. The Vite plugin recompiles on `pnpm dev` / `pnpm build`; for the
   standalone gates run `pnpm messages` (also wired as a pre-hook before
   `pnpm check` / `lint` / `test`).
3. Crowdin syncs `en.json` → `de.json` (and future locales). See the
   repo-root `crowdin.yml`. Untranslated keys fall back to English at
   runtime, so a partial translation is always safe to ship.

## Adding a language

1. Add the locale code to `project.inlang/settings.json` → `locales`.
2. Add `messages/<code>.json` (copy `en.json` as a starting point, or let
   Crowdin create it). `Intl.DisplayNames` supplies the picker label
   automatically — no extra string needed.

## Conventions

- Brand / technical identifiers stay verbatim across languages: `gamerat`,
  `ratbagd`, `libratbag`, `KWin`, `Steam`/`Lutris`/`Heroic`/`Piper`, `DPI`,
  shell commands, file paths, and `app_id` / `WM_CLASS`.
- Keyboard key names are **not** translated (see `keycode-map.ts`).
- The leading `$schema` key is metadata, not UI copy — leave it untouched.
