/**
 * Map a ratbagd `Device.Model` string (e.g. `"usb:046d:c08b:0"`) to
 * the corresponding mouse SVG filename, by parsing the
 * `svg-lookup.ini` file inherited from libratbag/piper.
 *
 * The ini format:
 *
 *   [Logitech G502]
 *   DeviceMatch=usb:046d:c08b;usb:046d:c08c
 *   Svg=logitech-g502.svg
 *
 * Multiple bus:vid:pid entries per section, semicolon-separated.
 * Piper's convention: when the `version` field is 0 (virtually all
 * devices), it's stripped from the key — we mirror that.
 */

interface LookupEntry {
    readonly section: string;
    readonly matches: readonly string[];
    readonly svg: string;
}

let cache: Promise<readonly LookupEntry[]> | null = null;

async function loadLookup(): Promise<readonly LookupEntry[]> {
    cache ??= (async () => {
        const res = await fetch('/mice/svg-lookup.ini');
        if (!res.ok) {
            console.warn('svg-lookup.ini not found, mouse SVG lookups will all return fallback');
            return [];
        }
        return parseIni(await res.text());
    })();
    return cache;
}

/**
 * Resolve a ratbagd `model` string to the SVG filename to load.
 * Falls back to `fallback.svg` (generic mouse outline) when no
 * specific match exists.
 */
export async function lookupMouseSvg(model: string): Promise<string> {
    const entries = await loadLookup();

    // Drop ":0" version suffix on the common case.
    const parts = model.split(':');
    const normalized = parts.length === 4 && parts[3] === '0' ? parts.slice(0, 3).join(':') : model;

    for (const entry of entries) {
        if (entry.matches.includes(normalized) || entry.matches.includes(model)) {
            return entry.svg;
        }
    }
    return 'fallback.svg';
}

function parseIni(text: string): readonly LookupEntry[] {
    const out: LookupEntry[] = [];
    let section: string | null = null;
    let matches: string[] = [];
    let svg = '';

    const commit = (): void => {
        if (section !== null && matches.length > 0 && svg.length > 0) {
            out.push({ section, matches, svg });
        }
    };

    for (const raw of text.split('\n')) {
        const line = raw.trim();
        if (line.length === 0 || line.startsWith('#') || line.startsWith(';')) {
            continue;
        }
        if (line.startsWith('[') && line.endsWith(']')) {
            commit();
            section = line.slice(1, -1);
            matches = [];
            svg = '';
            continue;
        }
        const eq = line.indexOf('=');
        if (eq === -1) continue;
        const key = line.slice(0, eq).trim();
        const value = line.slice(eq + 1).trim();
        if (key === 'DeviceMatch') {
            matches = value.split(';').map((s) => s.trim()).filter((s) => s.length > 0);
        } else if (key === 'Svg') {
            svg = value;
        }
    }
    commit();
    return out;
}
