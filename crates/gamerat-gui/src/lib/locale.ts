/**
 * Thin typed wrapper over Paraglide's generated locale runtime, in the
 * spirit of `theme.ts`.
 *
 * Paraglide owns the heavy lifting: the compile-time `strategy`
 * (`localStorage` → `preferredLanguage` → `baseLocale`) resolves the active
 * locale on load, persists the user's choice, and falls back to English for
 * untranslated keys. This module just exposes a small, stable surface for
 * the Settings language picker and keeps the rest of the app off the
 * generated `runtime.js` import path.
 *
 * Adding a language is config-only: add it to `project.inlang/settings.json`
 * `locales` and drop in `messages/<code>.json` — `LOCALES` and the option
 * labels below pick it up automatically (labels via `Intl.DisplayNames`).
 */

import { getLocale, isLocale, locales, setLocale, type Locale } from './paraglide/runtime.js';

export type { Locale } from './paraglide/runtime.js';

/** Supported locales, straight from the inlang project settings. */
export const LOCALES: readonly Locale[] = locales;

/** Crowdin project — where the unverified-translation notice funnels
 *  contributors to translate / proofread. */
export const CROWDIN_URL = 'https://crowdin.com/project/gamerat';

/** Locales whose translations a speaker has reviewed and signed off on.
 *  Everything else is shown as a "community" translation (typically
 *  machine / partial — Paraglide falls back to English per missing key).
 *
 *  A language graduates to verified by a PR adding it here — that review
 *  is the deliberate finalization checkpoint; this list is intentionally
 *  hand-maintained rather than derived from a Crowdin completion %. */
export const VERIFIED_LOCALES: readonly Locale[] = ['en', 'de'];

/** Whether `locale` is a verified (reviewed) translation. */
export function isVerified(locale: Locale): boolean {
    return VERIFIED_LOCALES.includes(locale);
}

/** The active locale, resolved by Paraglide's strategy chain. */
export function currentLocale(): Locale {
    return getLocale();
}

/** Endonym for a locale (its name in its own language): `en` → "English",
 *  `de` → "Deutsch". Derived via `Intl.DisplayNames` so new locales need no
 *  hand-written label; falls back to the upper-cased code. */
export function localeLabel(locale: Locale): string {
    try {
        const name = new Intl.DisplayNames([locale], { type: 'language' }).of(locale);
        if (name !== undefined && name.length > 0) {
            return name.charAt(0).toUpperCase() + name.slice(1);
        }
    } catch {
        /* Intl unavailable / unknown code — fall through. */
    }
    return locale.toUpperCase();
}

/** Switch the UI language. Paraglide persists the choice (localStorage) and
 *  triggers a full reload so every `m.*()` re-evaluates in the new locale —
 *  the same kind of reload the app already supports via Ctrl/Cmd+R. No-op
 *  when the value is already active or not a supported locale. */
export function changeLocale(next: string): void {
    if (!isLocale(next) || next === getLocale()) return;
    // setLocale may be async under some strategies; we don't await — its
    // default behaviour is a full page reload, so nothing runs after it.
    void setLocale(next);
}
