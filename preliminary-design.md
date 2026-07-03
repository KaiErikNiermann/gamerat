# Preliminary Design: A Rust libratbag & the Future Shape of the gamerat Stack

> **Status:** Preliminary scan / first ideas. Uncommitted, local-only. Not a
> commitment to build anything — a map of the terrain and a set of opinions to
> argue with. Written 2026-07-02.

---

## 0. Why we're even considering this

The immediate trigger is the multi-key macro limitation we hit while fixing
profile switching:

- libratbag's **only** macro write path is `ratbag_action_keycode_from_macro`,
  which hard-rejects any macro with more than one non-modifier key
  (`if (ratbag_action_macro_num_keys(action) != 1) return -EINVAL;`). It
  collapses "macro" down to *one key + modifier flags*. That's a keyboard
  shortcut, not a macro.
- The G502 HERO firmware (feature `0x8100` onboard profiles) demonstrably
  stores richer button state than libratbag will write. We are leaving device
  capability on the table.
- Worse, an unwritable macro used to poison the **entire** `Device.Commit`
  (the `-EINVAL` → `-1000` abort that broke all profile switching). We patched
  around it in `gamerat-ratbag` by writing such macros as disabled — a
  workaround for a limitation we don't control.

Beyond macros, the deeper motivation is **owning the stack**. Right now the two
layers below us (libratbag + ratbagd) are:

- Single-maintainer, maintenance-mode upstream (see memory:
  `libratbag-upstream-velocity` — ~6-month sweep cycles, patches carried
  indefinitely).
- Written in C with a driver model that's hard to extend for the things we
  actually want (real macros, richer onboard state, faster/cleaner commits).
- Already patched by us in two places (`RefreshActive`,
  `cheap-set-active-profile`) that we have to rebuild + `sudo meson install` +
  restart by hand on every machine.

The premise of gamerat — *push as much as possible onto the mouse's real
firmware* rather than faking it with software-defined input — is exactly the
premise that a better device layer would serve. So the question isn't "should
we escape software macros" (we already do, via onboard profiles); it's "is the
C device layer we're standing on the right one to keep standing on."

**This doc does not conclude "yes, rewrite now."** It concludes "here is the
shape a rewrite would take, here's the cheapest first slice that de-risks it,
and here's what the role boundaries should be so we don't paint ourselves into
a corner."

---

## 1. The current three-layer stack (as it actually is)

```
┌──────────────────────────────────────────────────────────────┐
│  gamerat-gui (Tauri + Svelte 5)                              │  user session
│    Tauri commands → gamerat-daemon over D-Bus (GameRat1)     │
├──────────────────────────────────────────────────────────────┤
│  gamerat-daemon  (org.appulsauce.GameRat1)      ~5.9k LOC     │  user session
│    focus → rule → SlotAllocator → device writes              │
│    soft-macros (uinput), dpi_tracker, game scanners          │
│    depends on: gamerat-ratbag, gamerat-focus, gamerat-input  │
├──────────────────────────────────────────────────────────────┤
│  gamerat-ratbag  (D-Bus CLIENT of ratbagd)      ~2.1k LOC     │  user session
│    proxy.rs  — zbus proxies (Manager/Device/Profile/...)     │  ← THE SEAM
│    client.rs — apply_profile_dpi / _complete, refresh_active │
│    button.rs — (uv) Mapping encode/decode, macro gating      │
├──────════════════════════ D-Bus (system bus) ════════════════┤
│  ratbagd  (org.freedesktop.ratbag1)             ~4.8k LOC C   │  system daemon
│    thin sd-bus translation, udev hotplug, async commit queue │  (root / hidraw)
│    +0001 RefreshActive  +0002 cheap-set-active  (our patches) │
├──────────────────────────────────────────────────────────────┤
│  libratbag  (static lib, linked into ratbagd)   ~34k LOC C    │  system daemon
│    driver vtable × 15 drivers (~14k LOC)                     │  (root / hidraw)
│    hidpp10/hidpp20/hidpp-generic transport (~a few k LOC)     │
│    libratbag-data (121 .device data files)                    │
└──────────────────────────────────────────────────────────────┘
```

### Roles, honestly stated

| Layer | Real responsibility | Privilege | Who touches hardware |
|-------|---------------------|-----------|----------------------|
| **libratbag** | Device semantics: HID++ transport, per-model drivers, device data/quirks, encode/decode of profiles-buttons-LEDs-DPI to the wire | needs `hidraw` (root) | **yes** |
| **ratbagd** | D-Bus surface + object lifecycle + udev hotplug + privilege boundary + commit scheduling. *Almost no logic of its own* — it's a translation layer. | runs as system daemon | via libratbag |
| **gamerat-daemon** | The actual product: focus→profile rules, LRU hardware-slot allocation, soft-macros, game DB, on-device DPI tracking | user session | no (delegates down) |
| **gamerat-ratbag** | Adapter: turns gamerat's `GameratProfile`/`ButtonAction` model into ratbagd D-Bus calls | user session | no |

Two things fall out of this table immediately:

1. **The privilege boundary is real and load-bearing.** libratbag needs
   `hidraw` access, which means root or a udev `uaccess` grant. gamerat-daemon
   needs the *opposite* — it must run in the user session to see focus
   (X11/Wayland/KWin), read `$XDG_CONFIG_HOME`, and drive `uinput`. This split
   is not an accident of history; it's a genuine trust/scope boundary. **Any
   rewrite must preserve it**, which strongly implies the device layer stays a
   separate privileged process (or becomes a carefully-scoped library loaded
   behind a udev grant — see §5.3).

2. **ratbagd is thin; libratbag is the moat.** ratbagd is ~4.8k LOC of
   mechanical sd-bus glue. libratbag is ~34k LOC, of which ~14k is *drivers* —
   15 vendors' worth of reverse-engineered protocol knowledge and 121 device
   data files. **The moat we cannot cheaply reproduce is driver breadth**, not
   the daemon.

---

## 2. What "rewrite libratbag in Rust" actually costs

Break the ~34k C LOC into what we'd have to reproduce:

| Chunk | ~LOC | Reproduce cost | Notes |
|-------|------|----------------|-------|
| HID++ 2.0 transport + `hidpp20` feature layer | few k | **Medium** | Well-specified, we already understand `0x8100`. High value — covers all modern Logitech. |
| HID++ 1.0 (`hidpp10`) | ~2.8k | Low priority | Legacy receivers; skip initially. |
| `hidpp20` **driver** (`driver-hidpp20.c`) | ~1.8k | **Medium** | This is the one that matters for our hardware. |
| Other 14 drivers (sinowealth, steelseries, roccat, asus, gskill…) | ~12k | **High (the moat)** | Each is a separate protocol RE effort. We have none of this hardware. |
| `libratbag-data` + 121 `.device` files | ~1k + data | **Low–Medium** | Data is portable; parser is easy. The data files themselves are the asset — reuse them verbatim. |
| Core model (context/device/profile/resolution/button/led structs) | ~2k | **Low** | Maps cleanly to idiomatic Rust types we mostly already have in `gamerat-proto`. |

**Takeaway:** a *complete* replacement is a multi-year, multi-hardware
undertaking dominated by driver breadth we can't test. A **hidpp20-only**
replacement — the protocol our hardware and most modern Logitech mice speak — is
a *few thousand LOC of medium-difficulty, well-understood* work. That's the
slice worth scoping.

The 121 `.device` data files are the one piece of the moat that's **portable**:
they're INI-ish text (`DeviceMatch`, `Driver`, button/LED/DPI counts, quirks).
A Rust rewrite should **read the exact same files** (or a lightly-transformed
copy), not re-derive them. That preserves most of libratbag's device-coverage
value for the drivers we do implement.

### 2a. Driver anatomy — how much of the ~14k is actually generalizable?

We deep-read 11 of the 15 drivers (roccat ×3, etekcity, sinowealth ×2, gskill,
hidpp20, steelseries, asus) and categorized the LOC. The headline: **"14k LOC of
drivers" is not one thing — it's ~50% collapsible boilerplate and ~50%
irreducible per-device protocol knowledge, and the split varies wildly by
driver.**

**The ~50% that Rust collapses breaks into three buckets, each with a specific
tool:**

| Bucket | What it is | ~% of a driver | Rust lever |
|--------|-----------|----------------|-----------|
| Lifecycle skeleton | `probe → discover caps → read → decode → commit dirty → set active` — structurally identical across *every* driver | 20–28% | one `trait Driver` with default methods, written once, not per-driver |
| Byte pack/unpack | manual field-by-field `buf[idx] = value` (de)serialization of profiles/buttons/DPI/LED | 17–45% | derive serialization (`binrw`/`deku`/`zerocopy`) → ~50–80% off |
| Mapping tables + lookups | device-code ↔ ratbag-action tables + the 2× linear-search fns repeated to use them | 8–17% | one declarative `button_map!` macro → bidirectional lookup |

Plus **outright duplication**: the three roccat drivers triplicate ~180 LOC of
protocol glue *byte-for-byte* (`roccat_is_ready`, `wait_ready`, `compute_crc`,
`set_config_profile` are 100% identical across `roccat.c`, `roccat-kone-emp.c`,
`roccat-kone-pure.c`). A shared crate deletes that on contact.

**The ~50% that does NOT shrink** is genuine reverse-engineering, ~200–300 LOC
per driver: sensor-specific DPI scaling (PMW3360 `−1`, PMW3389 `÷100`),
LED-effect matrices (sinowealth has **11** distinct RGB modes), macro wire
encoding (press/release as bitmasks + delay overlaid on the prior event),
checksum algorithms, and vendor quirks (G502 `INDEX_OFFSET`, G602 DPI `0xe000`
offset). This is language-independent — it's *knowledge*, not syntax.

**Why the reduction ratio swings so hard — the key nuance:**

| Driver | C LOC | ~Rust LOC | Reduction | Why |
|--------|-------|-----------|-----------|-----|
| roccat.c | 837 | ~350 | ~58% | boilerplate + triplicated glue |
| roccat-kone-emp.c | 1083 | ~405 | ~63% | ditto + LED tables |
| roccat-kone-pure.c | 843 | ~350 | ~58% | near-fork of roccat.c |
| etekcity.c | 715 | ~285 | ~60% | simple protocol, no checksum |
| steelseries.c | 1123 | ~280 | ~65% | protocol-version switch ladders → enum type param |
| asus.c | 667 | ~330 | ~50% | multi-profile orchestration is inherent |
| **hidpp20 driver** | 1804 | ~650 | ~50% | already orchestration over factored `hidpp20.c` transport |
| gskill.c | 1508 | ~1020 | ~33% | feature-rich: 5 profiles + full macro read/write |
| sinowealth.c | 2192 | ~1400 | ~36% | genuinely feature-rich: 11 LED effects, multi-sensor DPI |

Two regimes fall out:

- **"Thin" drivers** (roccat/etekcity/steelseries/asus): 50–65% reduction. Mostly
  boilerplate, duplication, and a simple protocol. Rust crushes them.
- **"Fat" drivers** (sinowealth, gskill): only 33–36%. They're not big because
  they're *verbose* — they're big because those devices genuinely *do more* (11
  RGB effects, 34 button mappings, multi-sensor DPI). That surface area is real
  RE and survives the rewrite.

**hidpp20 is the best-case driver for us** — ~50% at the driver level *because*
libratbag already factored transport into `hidpp20.c`/`hidpp-generic.c`, so the
driver is mostly orchestration. That's exactly the shape a Rust `trait Driver`
over a shared `hidpp20` transport crate wants; our own driver benefits most.

**Aggregate:** the 11 analyzed drivers total ~11,100 C LOC → ~5,300 Rust
(~52%). Extrapolated to all ~14k of drivers, very roughly **~6.5–7k Rust LOC** —
*if* every driver were reimplemented, which we are explicitly not proposing.

**The load-bearing caveat:** LOC reduction was never the moat. The moat is the RE
knowledge in the irreducible ~50% **plus the 121 `.device` files — and neither
shrinks.** Rust makes drivers *we already understand* (hidpp20) far cleaner and
more maintainable; it does **not** let us support the other 13 vendors without
their hardware. This *reinforces* the strategy below rather than changing it:
hidpp20-first, reuse the data files verbatim, keep the C stack as fallback for
everything we can't test.

---

## 3. The seam that makes this tractable

The single most important architectural fact for planning a rewrite:

> **gamerat's entire dependency on the C stack passes through one narrow,
> well-defined interface — the ratbagd D-Bus surface — and one adapter crate,
> `gamerat-ratbag` (~2.1k LOC, thick part ~1.5k in `client.rs`+`button.rs`).**

`gamerat-daemon`, `gamerat-proto`, `gamerat-cli`, `gamerat-focus`,
`gamerat-input`, `gamerat-gamedb` know **nothing** about ratbagd. They call
`RatbagClient` methods (`apply_profile_dpi`, `apply_profile_complete`,
`refresh_active`, `list_buttons`, `set_button_on_profile`, …). That means we can
replace everything below `gamerat-ratbag` **without touching the product**, as
long as we preserve either:

- **(a) the D-Bus contract** (`org.freedesktop.ratbag1`, the object paths, the
  `(uv)` Mapping shape, `Commit`/`SetActive`/`RefreshActive`), so the existing
  `proxy.rs` keeps working; **or**
- **(b) the `RatbagClient` Rust API**, by swapping `gamerat-ratbag`'s internals
  from "zbus calls to ratbagd" to "direct calls into a Rust device crate."

This is the fork in the road, and it's the key decision. §5 lays out the routes.

---

## 4. The multi-key macro question, specifically

Worth pinning down because it's the concrete trigger and a good litmus test for
"is a rewrite worth it."

- **Is it a firmware limit or a libratbag limit?** Firmware side: the G502's
  `0x8100` onboard macro storage *can* hold multi-step sequences (Logitech's own
  software writes them). libratbag side: the *write path* was never generalized
  beyond single-key-plus-modifiers. So it's **primarily a libratbag limitation**,
  with the caveat that the exact multi-step encoding per device is
  reverse-engineered and under-documented — which is *why* upstream never did it.
- **What a Rust rewrite unlocks:** a proper `hidpp20` macro encoder that writes
  the full `MacroStep[]` sequence to onboard memory, so the "chain of keys" UX
  the user wants lives on the mouse, not in our `uinput` soft-macro fallback.
- **The catch:** this is exactly the part with the least upstream reference code
  and the highest RE risk. It should be an **early spike** (prove we can write +
  read back a 3-key macro on the G502) precisely *because* it's the load-bearing
  justification. If that spike fails, much of the "rewrite for macros" rationale
  weakens and we'd lean toward keeping soft-macros for sequences.

---

## 5. Routes forward

### 5.1 Route A — Do nothing structural; keep patching C (baseline)

Keep libratbag+ratbagd, keep carrying patches, add a third patch for multi-key
macros if we can figure out the encoding.

- **Pros:** cheapest; retains all 15 drivers / 121 devices for free; no privilege
  rework.
- **Cons:** we're writing the hard part (multi-key `hidpp20` macro encoding) in
  C, in someone else's codebase, that we rebuild-by-hand everywhere; every
  improvement is a carried patch; the maintenance-mode upstream won't take them
  fast (or at all).
- **Verdict:** fine as the *status quo floor*. The macro encoder work is the same
  RE difficulty in C or Rust, but in C we get none of the ownership/ergonomics
  upside. If we're going to do that hard work anyway, do it in Rust.

### 5.2 Route B — Clean-room Rust device crate, hidpp20-first, behind the existing `RatbagClient` API  ⭐ recommended direction

Build a new crate — call it **`gamerat-device`** (or `ratbag-rs`) — that talks
to `hidraw` directly and implements *just* the hidpp20 path first. Expose it in
Rust behind the **same trait** `gamerat-ratbag` already presents to the daemon.
Make the backend **runtime-selectable**:

```
gamerat-daemon
      │  (unchanged: calls RatbagClient trait)
      ▼
gamerat-ratbag  ──►  enum Backend {
                        Ratbagd(existing zbus proxies),   // fallback: all other devices
                        Native(gamerat-device),           // hidpp20 devices we support
                     }
```

- For a **supported hidpp20 device** (G502 &co), use the native Rust path —
  full multi-key macros, our own fast commit, no ratbagd, no carried patches.
- For **anything else**, fall back to ratbagd/libratbag unchanged, so we never
  regress device coverage.
- **Reuse the 121 `.device` data files** for match/quirk/counts.

**Pros:** de-risked and incremental — ship value on *our* hardware immediately,
grow the supported-device set driver by driver, never lose the C fallback. The
product layer (`gamerat-daemon` and up) doesn't change. We own the macro path.

**Cons:** we now maintain two backends during the transition; the privilege
question (§5.3) has to be answered for the native path.

**Why this over Route C:** keeping the *Rust* API as the seam (not the D-Bus
contract) lets the native path be a plain in-process library call with no IPC,
and frees us from bug-for-bug reimplementing ratbagd's D-Bus quirks.

### 5.3 The privilege question for the native path (must-answer for Route B)

The native crate needs `hidraw` access. Three options, in increasing order of
"nice but more work":

1. **Keep a thin privileged system daemon** ("`gamerat-devd`") that wraps
   `gamerat-device` and exposes a minimal IPC — essentially a Rust ratbagd, but
   *our* interface, only as wide as we need. Preserves today's trust boundary
   exactly. Most conservative. This is the natural place the "rewrite ratbagd"
   energy should go — **not** a clone of `org.freedesktop.ratbag1`, but a
   purpose-built minimal device service.
2. **udev `uaccess` grant** on the specific hidraw node so the *user-session*
   gamerat-daemon can open the device in-process — **no system daemon at all**.
   Cleanest end-state (one process owns the whole device story), but widens
   hidraw access to the user's session (any session process could poke the
   mouse). Acceptable for a single-user desktop; document the tradeoff.
3. **Polkit-gated helper** — middle ground, more machinery than it's worth for a
   single-user tool right now.

**Recommendation:** prototype against option 2 (`uaccess`, in-process) for speed
of iteration on the G502, but architect `gamerat-device` so it can be hosted by
a privileged `gamerat-devd` (option 1) for the shipped product. Keep the device
crate *transport-agnostic and privilege-agnostic* — it just needs an open fd.

### 5.4 Route C — Rust ratbagd clone that claims `org.freedesktop.ratbag1`

Reimplement ratbagd's D-Bus surface in Rust so `proxy.rs` is unchanged.

- **Pros:** zero change in `gamerat-ratbag`; could theoretically serve *other*
  ratbagd clients (piper) too.
- **Cons:** we'd be reimplementing a *compat surface* we don't love, inheriting
  its object model and the `(uv)` marshalling warts, and for non-hidpp20 devices
  we'd have nothing to serve (can't proxy to libratbag without linking it). Only
  makes sense if being a drop-in `org.freedesktop.ratbag1` provider for the
  broader ecosystem is a goal. **It isn't, for us.** Skip.

### 5.5 Route D — FFI-wrap libratbag, strangle it driver-by-driver

Use `bindgen` over libratbag and replace pieces from the Rust side.

- **Reality check:** libratbag ships as a **static** lib with a tightly-coupled
  internal driver vtable (`struct ratbag_driver` fn-pointers over private
  structs). FFI at that boundary means implementing C fn-pointers from Rust over
  opaque C state — high friction, and we'd still be linking the whole C context
  model. bindgen buys us little because the *hard* part (hidpp20 macro encoding)
  isn't exposed at a clean C boundary anyway.
- **Verdict:** not worth it as the primary strategy. **Narrow exception:** FFI
  could be a *temporary bridge* to borrow a single hard-to-RE driver we lack
  hardware for — but that's a corner case, not the plan.

---

## 6. Should ratbagd merge into gamerat-daemon?

Short answer: **no — keep the device layer as a separate concern from the
product daemon, but rewrite it as ours.**

Reasoning:

- The two daemons sit on **opposite sides of the privilege boundary** (§1). The
  device layer wants root/hidraw and to be dumb about focus; gamerat-daemon
  wants the user session and knows nothing about HID++. Merging forces one of
  them across the boundary — either gamerat-daemon runs as root (bad: it drives
  uinput, reads focus, watches XDG config — huge attack surface as root), or the
  device code runs unprivileged (needs the uaccess grant anyway, at which point
  it's a *library*, not a merged daemon).
- The clean move is: **device layer = a Rust *library* (`gamerat-device`)**,
  optionally hosted by a **minimal privileged service (`gamerat-devd`)** if we
  keep the boundary as a process split. gamerat-daemon consumes the library API
  (directly, if uaccess; via thin IPC, if devd). Either way the *product* logic
  stays exactly where it is.

Target end-state role map:

| Component | Role | Privilege | Fate |
|-----------|------|-----------|------|
| `gamerat-gui` | UI | user | unchanged |
| `gamerat-daemon` | rules/focus/slots/soft-macros — the product | user | unchanged |
| `gamerat-ratbag` | backend selector: Native vs Ratbagd | user | thins out; becomes the enum in §5.2 |
| **`gamerat-device`** (new) | HID++ transport + drivers + device data, in Rust | agnostic (needs an fd) | **the rewrite** |
| **`gamerat-devd`** (maybe) | minimal privileged host for the above | system | only if we keep a process split |
| ratbagd + libratbag (C) | fallback for unsupported devices | system | retained during transition, retired per-driver |

---

## 7. Suggested phasing (de-risk before commit)

1. **Spike 0 — prove the macro premise (days).** Outside the workspace, in a
   throwaway Rust bin: open the G502 hidraw, speak `0x8100`, and **write + read
   back a genuine 3-key onboard macro**. This is the single fact the whole
   rewrite rationale rests on. If it works, proceed. If it doesn't, Route A +
   soft-macros for sequences is the honest answer.
2. **Spike 1 — read path parity.** In `gamerat-device`: enumerate the G502,
   parse its `.device` data file, read profiles/buttons/DPI/LEDs, and diff the
   decoded model against what ratbagd currently reports. Proves our decode
   matches reality.
3. **Phase 2 — write path + backend seam.** Implement commit/set-active/DPI/
   button/LED writes; wire `gamerat-ratbag` as the `enum Backend` selector
   (§5.2) with Native for the G502 and Ratbagd fallback for all else. Ship
   behind a flag. Fold our two carried patches' behavior in natively (cheap
   set-active is just "don't flash unless data changed"; RefreshActive is a
   `0x8100` current-profile/DPI read).
4. **Phase 3 — privilege story.** Decide uaccess vs `gamerat-devd`; harden.
5. **Phase 4+ — grow drivers** only as real hardware/testers appear. Never
   remove the C fallback until a driver is proven.

Each phase is independently shippable and leaves the product working. Nothing
here requires a big-bang cutover.

---

## 7a. Validating drivers you can't hold — the C as a runtime oracle

The uncomfortable truth: we own exactly one of the ~15 supported device families
(the G502, hidpp20). For every other driver, the **C source is our only spec** —
we can't plug in the mouse to check. The naive framing of this is "carefully
translate the C and pray it works because the same thing works in C." That
framing is *half right and quietly dangerous*, for three reasons — and the third
one is also the fix.

**(1) The real spec is the byte layout, not the logic.** The RE knowledge lives
in the *exact wire bytes*: `#[repr(C, packed)]` structs, unions, bitfield order,
padding, endianness, integer promotion. You can port the control flow perfectly
and still emit wrong bytes. The bug surface for a translation is the **ABI**, not
the algorithm — and the ABI is the part you can't see by reading.

**(2) The oracle is itself partly wrong.** The drivers contain literal
`/* There is almost no chance this is correct */` (gskill button mapping) and
FIXME-riddled tables (roccat-kone-emp). A faithful translation faithfully
reproduces those bugs. GitHub issues are the **errata sheet** for the oracle —
they mark which parts are known-broken. Improving past them is genuine RE, not
translation, and is *not* validatable without the hardware. So: faithful port of
*working* C = inherit working behavior; faithful port of *guessed* C = inherit
the guess. Know which you're doing.

**(3) The C is a *runtime* oracle, not just a reading oracle — this converts
most of the prayer into a CI check.** We can't test against the mouse, but we can
test against the running C, which *is* the RE knowledge made executable:

- **FFI differential testing (gold standard).** Link the actual C driver, feed
  the C encoder and the Rust encoder the *same* input model, assert
  **byte-for-byte identical output**. Any divergence is a translation bug caught
  mechanically, zero hardware. (This is `fragment-detector`'s
  `verify_equivalence` idea scaled to a driver.) It proves *fidelity to the
  oracle* — it cannot catch bugs the oracle itself has.
- **Golden / replay vectors.** Capture real HID traffic *once* — `usbmon` /
  Wireshark, libratbag's own `hidpp20-*` debug tools, or a single capture from
  anyone who owns the device — and freeze it as encode/decode fixtures.
- **Virtual device.** libratbag ships a test-driver harness (`driver-test.c`);
  the same shape lets us replay recorded exchanges in CI without hardware.

**Resulting confidence tiers:**

| Tier | Validation available | Confidence |
|------|----------------------|-----------|
| hidpp20 (we own a G502) | real device | High — full runtime test |
| drivers with FFI-diffable C / capturable traffic | byte-equality vs C oracle | Medium-high — proves translation fidelity; can't catch the C's own bugs |
| drivers whose C is marked "probably wrong" | none | Inherit the C's uncertainty — no worse, no better |

**The escape hatch (already in the design):** under Route B we are *never forced*
to port a driver we can't validate. The C fallback stays; porting is opt-in,
per-device, and **gated on a validation path existing** — real hardware (us, for
hidpp20), a byte-diff harness, or a contributor with the device. "Translate and
pray" only becomes mandatory if we choose to *delete* the C dependency wholesale,
which is a late, optional goal — not a prerequisite for any shippable phase.

**Practical rule:** don't hand-port a driver and eyeball it. For anything beyond
hidpp20, stand up the FFI byte-equivalence harness *first* and let it gate the
port. That's the difference between "port drivers by faith" and "port drivers
behind a proof."

---

## 7b. Accelerating & validating RE — a research scan (automata learning, format inference, meta-drivers)

The question behind this section: can we use *automated* RE techniques — including
a CEGAR-style "abstract the driver, refine on new data" loop — to speed up or
validate porting, and can we design the driver interface so the framework itself
*facilitates* making/testing new RE'd drivers (a "meta-driver")? We searched the
literature and the neighbouring open-source projects. Three techniques apply, at
very different confidence levels, plus one clear substrate for the meta-driver.

The reframing that governs all of it: **because we hold the C source, we are
already in the "white-box" regime.** Most automated PRE tooling exists to recover
structure we *already have documented in the C driver*. So these techniques are
not a shortcut around reading the C — they are **verification, gap-finding, and
bootstrapping** instruments layered on top of it.

### 7b.1 Message-format inference (the read side) — secondary, but three real uses

Trace-based format-inference tools infer field boundaries/types/length/checksum
from captured messages. Classic (execution-taint) line: **Polyglot**, **Tupni**,
**AutoFormat**; modern binary revivals **BinPRE** (CCS 2024), **ICEPRE** (2025).
Black-box (trace) line: **Discoverer**, **FieldHunter**, **PRISMA**; usable today
are **[Netzob](https://github.com/netzob/netzob)** (the interactive workbench —
explicitly targets "communication with drivers and devices", handles binary,
works offline; semi-dormant but installable), **[NEMESYS](https://www.usenix.org/conference/woot18/presentation/kleber)**
(infers boundaries from value-change *within a single message*, so it tolerates
*low* message counts — relevant: we have few, short reports), and
**[BinaryInferno](https://github.com/binaryinferno/binaryinferno)** (NDSS 2023,
fully automatic ensemble for length fields, floats, entropy boundaries in packed
binary).

Surveys: [Narayan et al. 2015](https://dl.acm.org/doi/10.1145/2840724),
[Duchêne et al. 2018](https://link.springer.com/article/10.1007/s11416-016-0289-8)
(cleanest taxonomy: network-trace vs execution-trace), index at
[techge/PRE-list](https://github.com/techge/PRE-list).

**Honest verdict for us:** *not* the primary tool — the C struct *is* the format
these tools spend effort recovering. Its narrow, real value:
- **(a) sanity-check our reading of the C** — run captured reports through
  BinaryInferno/Netzob, confirm inferred offsets/endianness match our struct;
- **(b) catch fields the C ignores** (highest value) — entropy/value-change
  analysis can flag a "reserved" byte that actually *varies* on the wire,
  surfacing behaviour the C never modelled;
- **(c) bootstrap a device with *no* driver** — the legitimate greenfield use.

**Checksum inference specifically** (our CRC problem) is the one transferable
gem: **[Chandler et al., "Automatic Discovery and Synthesis of Checksum
Algorithms from Binary Data Samples," PLAS 2020](https://www.eecs.tufts.edu/~chandler/checksums2020.pdf)**
— locates the checksum in a *small* set of messages and *program-synthesizes the
algorithm* (which CRC variant, over which byte range). Worth a dedicated step
when RE'ing a new device's checksum.

### 7b.2 Automata learning + CEGAR (the sequencing side) — your idea, honestly scoped

This is the direct hit for the CEGAR intuition, and it is a *real, published*
line — not a novelty. **Active automata learning** (Angluin's L\*, modern **TTT**;
tools **[LearnLib](https://learnlib.de)**, **[AALpy](https://github.com/DES-Lab/AALpy)**)
infers a black box's **state machine** by querying it: membership queries (send a
command sequence, observe responses) build a hypothesis Mealy machine; an
*equivalence query* returns a **counterexample** that refines it. See Vaandrager's
[CACM "Model Learning" survey](https://cacm.acm.org/research/model-learning/).

The CEGAR connection is exact: plain L\* needs a *small finite* alphabet, but a
mouse command carries data (DPI values, profile indices, colours) → effectively
infinite alphabet. The Radboud/Vaandrager answer is literally titled
**["Automata Learning through Counterexample-Guided Abstraction Refinement" (Aarts,
Heidarian, Kuppens, Olsen, Vaandrager, FM 2012)](https://link.springer.com/chapter/10.1007/978-3-642-32759-9_4)**:
a *mapper* collapses the concrete alphabet to a small abstract one; when the
abstraction is **too coarse it induces nondeterminism**, and that nondeterminism
*is* the counterexample used to **automatically refine** the abstraction (split a
class, remember a value). Implemented in **[Tomte](http://tomte.cs.ru.nl)**;
successor is register-automata learning (**[RALib](https://github.com/LearnLib/ralib)**),
and — relevant because we *have* the C — **[grey-box register-automata learning](https://arxiv.org/pdf/2009.09975)**
that exploits source access. The proof it finds real bugs:
**[de Ruiter & Poll, "Protocol State Fuzzing of TLS," USENIX Security 2015](https://www.usenix.org/system/files/conference/usenixsecurity15/sec15-paper-de-ruiter.pdf)**
— learned Mealy models of nine TLS stacks and *diffed the state machines* to
expose flaws. Closest hardware analogue: [automata learning of BLE devices,
FMSD 2023](https://link.springer.com/article/10.1007/s10703-023-00425-y).

**Honest verdict for us — a scalpel, not a hammer:**
- **Genuinely useful for the *stateful sequencing* layer.** If a device gates
  writes behind an unlock/select handshake, or `commit` has ordering semantics,
  learning *recovers that FSM as a spec* and the equivalence query becomes the
  "did my Rust port diverge from the C in ordering?" oracle — exactly the
  TLS-diff pattern. This catches bugs no unit test enumerates.
- **Overkill for the stateless codec.** Pure request→response byte-packing has no
  interesting state; a round-trip property test (`decode(encode(x)) == x`) proves
  more, faster. Don't learn an FSM to rediscover "it's a function."
- **Two hard limits.** (1) Active learning needs a *queryable, resettable*
  SUT — so it only applies where we have **live hardware** (hidpp20/G502) or a
  faithful emulator; with only C + traces you're stuck with weaker *passive*
  learning (no equivalence query = no divergence oracle). (2) Destructive/flash
  writes make "reset between queries" unsound and wear the device — you'd learn
  against an emulator, not the real mouse. And HID-specific precedent is thin
  (done for TLS/BLE/EMV/TCP), so we'd be *adapting*, not following a paved path.

**So:** keep automata learning as an *optional, opt-in validation* for the small
stateful handshake portion of a driver — most valuable precisely for hidpp20's
`0x8100` onboard-profile state — and design the `Driver` trait so a driver *can*
expose a learnable/queryable surface, but don't make it load-bearing.

#### Where the device state actually is (so we know what's worth learning)

Statefulness in this domain does *not* live in the codec (packing DPI into bytes
is a pure function). It lives in a few specific protocol layers, and only some
are FSMs worth learning. Ranked by footing:

- **hidpp20 `0x8100` onboard layer — mode × active-profile × dirty. THE one.** The
  device genuinely holds: a **mode** (onboard `0x01` vs host `0x02`, flipped by
  `SET_ONBOARD_MODE 0x10`) that *gates how later commands behave*; an **active
  profile pointer** (`SET/GET_CURRENT_PROFILE`); and **RAM-vs-flash / dirty**
  state (edits are volatile until `commit` flashes them). **The switching bug that
  started this whole thread was a sequencing-state bug in exactly this layer** — a
  failed button write `-EINVAL` aborting the all-or-nothing commit and silently
  dropping the profile switch. A learned model of `{mode, active_profile,
  dirty_mask}` diffed between the C driver and our Rust port would have caught
  that divergence *mechanically*. And because the state is entangled with a data
  value (which profile index), this needs **register automata / the CEGAR mapper**
  (Tomte/RALib), not plain L\* — the "remember which index" requirement *is* the
  infinite-alphabet problem that line of work solves. If we demo learning
  anywhere, demo it here.
- **Paged / transactional writes — `select(page) → write chunks → commit`.**
  Order-dependent hidden state ("which page/bank is selected"): roccat's
  `set_config_profile` before read/write, kone-emp's two-bank macro split, gskill's
  `select_macro`/`select_profile` that "can't use the normal command handler."
  Out-of-order access silently corrupts. Data-parametric (the page index) → again
  register-automata territory.
- **Unlock / init handshakes** (sinowealth-nubwo's magic pre-query; steelseries
  gating on a firmware-version query) — real but usually *shallow* (2-3 states),
  so a hand-written FSM + round-trip test beats standing up LearnLib.
- **Wireless / Unifying receiver** (device-index multiplexing, pairing handshake —
  why `liblur` exists) — the *most* protocol-stateful thing in the ecosystem, but
  adjacent to mouse config; a legitimate target only if we touch pairing.
- **Input-report side** (a "profile cycle" button advancing active-profile mod K =
  a counter automaton; DPI-shift "held" state) — learnable from live hardware
  exactly like the BLE-device paper, but reads input reports, so a different
  experiment.

**Two caveats keep it a scalpel.** (1) Most of this state is shallow (2-4 states);
CEGAR/register-automata only earns its cost where **state entangles with a data
value** — active-profile index, selected page, dirty bitmask — which, notably,
`0x8100` does. (2) Active learning needs *live, resettable* hardware, and flash
writes make "reset between queries" both unsound and wearing, so the realistic
venue is **the G502 we own, driven against the record/replay emulator (§7b.4)**,
not real flash.

**Minimal experiment with genuine ground under it:** learn a register-automaton of
the G502's `0x8100` `{mode, active, dirty}` layer from *both* the C driver and the
Rust port, and diff them — a test that targets the precise bug class that started
this thread.

### 7b.3 The meta-driver — declarative codecs + data files + built-in validation

The neighbouring projects answer "how do you scale to many devices without 15
hand-coded modules," and they sort into a clear hierarchy:

- **Data-driven / protocol-as-capability (the goal):**
  **[Solaar](https://github.com/pwr-Solaar/Solaar)** implements Logitech *features*
  (`0x2201` DPI, etc.), not devices — a new mouse speaking known features costs
  **zero device code**. **[rivalcfg](https://github.com/flozz/rivalcfg)** describes
  each mouse as a *dict* of command-bytes/report-type/lengths/value-types consumed
  by one generic writer — the wire format *as data*. **libratbag's own
  [`.device` files](https://github.com/libratbag/libratbag/tree/master/data/devices)**
  make adding a same-protocol device a *data-only* change (but stop short of
  declaring the *wire format* as data — that's the gap to close).
- **Anti-patterns (breadth by brute force):**
  **[OpenRGB](https://github.com/CalcProgrammer1/OpenRGB)** — every device is a
  free-form C++ `Controller`+`Detector`; scales *socially*, not architecturally.
  **[OpenRazer](https://github.com/openrazer/openrazer)** — worst case, a new
  device edits *both* a kernel module and the daemon.
- **Layering lesson:** **[Piper](https://github.com/libratbag/piper)** is a pure
  GTK frontend with *zero* device knowledge — keep all device logic below the IPC
  line. (gamerat already does this; keep it.)

**The substrate — and the write path is the discriminator.** Most parser tech is
read-only: **[Kaitai Struct](https://kaitai.io)** only emits write code for
Java/Python (**no Rust write** — usable as a spec/scratch-doc language, not
runtime); **[Spicy](https://github.com/zeek/spicy)** (Zeek) is *parse-only*.
**The pick is [`deku`](https://github.com/sharksforarms/deku)**:
`#[derive(DekuRead, DekuWrite)]` gives **symmetric read+write**, **bit-level**
fields, and attributes for **checksums, conditional (`cond`) fields, and unions**
— packed structs + bitfields + CRCs + variant fields, *both directions, from one
declaration*. Complements: **[zerocopy](https://github.com/google/zerocopy)** for
fixed report headers, **[modular-bitfield](https://docs.rs/modular-bitfield)** for
dense bit packing, **[binrw](https://github.com/jam1garner/binrw)** as the
byte-aligned fallback (its bitfields are clunky).

**Recommended meta-driver shape** (this is the actionable design payoff):

1. **`trait Driver` over a shared HID/HID++ transport.** One transport +
   HID++ feature-negotiation layer reused across the whole Logitech family (the
   Solaar lesson). The trait stays thin: `capabilities()`, `read_profile()`,
   `apply(&Report)`.
2. **Wire formats as `deku` derives — the *only* per-protocol code.** Each
   protocol family is a small module of `#[derive(DekuRead, DekuWrite)]` structs;
   checksums/bitfields/unions are *declared*, yielding encode + decode +
   validation from one definition. This replaces libratbag's hand-coded C
   backends with ~tens of lines of declarative Rust **per protocol, not per
   device** — and directly attacks the "17–45% manual byte-packing" bloat from
   §2a.
3. **Device params in libratbag-style data files** (TOML/RON): `DeviceMatch`,
   DPI ranges, counts, and *which codec module* to bind. Same-protocol device =
   data-only change (Solaar/rivalcfg/libratbag model).
4. **Record/replay + byte-diff as a first-class trait capability, not a bolt-on.**
   Abstract the transport so it can be backed by *real hidraw*, a *replay
   fixture*, or an *FFI-to-C diff harness* interchangeably. Then "add a device"
   is: **(a)** write a `.device` data file, **(b)** if the protocol is new, add a
   thin `deku` codec, **(c)** drop in a captured trace — and CI proves the codec
   matches the wire (§7b.4).

### 7b.4 The capture / replay / diff harness — concrete, hardware-free

The tooling for §7a's "runtime oracle" already exists and is CI-proven:

- **Capture once:** **[hid-tools](https://gitlab.freedesktop.org/libevdev/hid-tools)**
  `hid-recorder` dumps a hidraw node's report descriptor + every report in a
  replayable trace; drive the exchanges with
  **[hidapitester](https://github.com/todbot/hidapitester)** / `ratbagctl`.
  libratbag *itself* ships a `ratbag.recorder`/`ratbag.emulator` pair and
  **[ratbag-emu](https://github.com/libratbag/ratbag-emu)** for exactly this.
- **Replay with no hardware:** **[`uhid`](https://docs.kernel.org/hid/uhid.html)**
  materializes a kernel HID device from userspace; `hid-replay` feeds a recorded
  trace through it. The **kernel HID subsystem's own pytest suite** (hid-tools)
  builds `UHIDDevice`s and asserts on outputs headless under CI — proof this is a
  legitimate driver-test methodology to mirror.
- **Differential codec check (no device at all):** FFI-link the C encoder
  (`bindgen`+`build.rs`), then **[proptest](https://crates.io/crates/proptest)**
  asserts `rust_encode(x) == c_encode(x)` byte-for-byte over generated configs
  (auto-shrinks divergences), with **[cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz)**
  differential targets for edge coverage.

Two-layer harness: **Layer A** (codec equivalence via FFI-diff + fuzz — strongest,
fastest, zero hardware) gates every port; **Layer B** (uhid replay of the frozen
trace, daemon talks to the virtual device, assert reports match golden + reads
round-trip). **Limits to document:** replay validates only the *byte encoding and
the req/resp mapping you captured* — it can't validate device-side state machines,
timing/retries, unrecorded config combinations, or firmware-version drift. Keep a
thin on-hardware smoke test for those; everything else runs in CI without a mouse.

### 7b.5 What this means for the interface, from day one

The RE-acceleration research doesn't change *whether* to rewrite — it changes what
we build into the driver interface *before* writing a second driver:

- **Codec = declarative (`deku`), symmetric, per-protocol.** Never hand-write
  byte-packing again; the derive *is* the spec and the round-trip test is free.
- **Transport is swappable** (hidraw / replay-fixture / FFI-C-diff) so record/
  replay and byte-equivalence are structural, not afterthoughts.
- **Data files carry device params**, codecs carry protocols — adding a device is
  data + a captured trace, not code.
- **Automata-learning stays an opt-in hook** for the small stateful handshake
  layer (hidpp20 `0x8100`), where a learned+diffed FSM earns its keep — not a
  requirement.
- **Format inference + checksum synthesis are RE-time aids** for greenfield
  devices, not runtime dependencies.

That's a driver framework where the *validation story is part of the type
signature*, which is exactly what makes porting the long tail tractable later.

---

## 7c. Scope horizon — tablets and other peripherals (should the interfaces accommodate them?)

The question: should gamerat expand beyond mice — to drawing tablets
(OpenTabletDriver) and other "gaming/creative" peripherals — and, more usefully
*now*, should the interfaces we're about to design be built to *accommodate* them
even if we don't build them yet? We surveyed the ecosystem. The answer splits the
whole space along one axis, and that axis is the thing to bake into the design.

### 7c.1 The axis that organizes everything: onboard-config vs software-driven

- **Onboard-config** (gamerat's current model): settings are written to device
  flash; the device applies them itself, host-independent. Profiles survive a
  reboot with no daemon running. This is libratbag/ratbagd.
- **Software-driven**: a persistent host daemon intercepts/processes input and
  *holds* the config; unplug the daemon and the behavior is gone. There is no
  device state to write — you reconfigure the daemon.

Every candidate peripheral sorts cleanly onto this axis:

| Class | Reference OSS | Model | Fit |
|-------|---------------|-------|-----|
| Mice | libratbag/ratbagd | **onboard** | current |
| **Keyboards / macropads (QMK)** | **VIA / [Vial](https://get.vial.today/)** | **onboard** | *strongest* analogue — same "write firmware + flip profile" pattern, real raw-HID config protocol, third-party clients already exist ([Pipette](https://github.com/darakuneko/pipette-desktop)) |
| **Drawing tablets** | **[OpenTabletDriver](https://github.com/OpenTabletDriver/OpenTabletDriver)** | **software-driven** | the user's ask; niche but same domain |
| Gamepads | Steam Input / [xpadneo](https://github.com/atar-axis/xpadneo) | software-driven (narrow onboard-slot exception on Xbox Elite 2) | later |
| Stream Decks | [deckmaster](https://github.com/muesli/deckmaster)/OpenDeck | software-driven | later |
| Razer keypads | OpenRazer + input-remapper | software-driven (no good onboard story) | avoid |

### 7c.2 OpenTabletDriver specifically — don't reimplement it, *drive* it

OTD is excellent and we should not reimplement it (agreed). Its architecture is
the crux: it is a **user-mode software input pipeline** (.NET/C#, LGPL-3.0,
Linux via evdev-in/uinput-out) that reads raw pen reports and does area-mapping,
pressure-curve, filtering, and binding resolution *entirely in host software*.
Tablets store essentially **nothing** relevant onboard — so a tablet "profile" is
pure host state, and there is **no SlotAllocator analogue** (the whole
hardware-slot allocation problem simply doesn't exist for tablets).

Two facts make integration clean:

1. **It's drivable by a third party.** `OpenTabletDriver.Daemon` exposes
   **JSON-RPC 2.0 over a named pipe** (StreamJsonRpc); the GUI and the `otd` CLI
   are both just clients of the public `IDriverDaemon` contract
   (`GetSettings`/`SetSettings`/`ResetSettings`, `preset`, `set-display-area`, …).
   `SetSettings` takes the *whole* Settings object (get → mutate profile → set).
   There is **no D-Bus and the maintainers won't add one** — so we bridge via
   named-pipe RPC or shelling to `otd`, not via our D-Bus bus, and *not* as an
   in-process plugin.
2. **It has native profiles but explicitly no per-app switching.** "Bindings per
   application" was [closed as not-planned](https://github.com/OpenTabletDriver/OpenTabletDriver/issues/1358);
   layered/auto-switch settings remain unshipped. **This is precisely gamerat's
   hole to fill**: OTD is the *mechanism* (swap settings via RPC), gamerat is the
   *policy* (which preset per focused app). The osu!-tight-area vs
   Photoshop-full-area case is a real, wanted workflow OTD can't do alone. Same
   `focus → rule → profile` shape as the mouse pipeline, just a different backend
   actuator.

### 7c.3 The design payoff — what to make accommodating *now* (build nothing yet)

We should **not build tablet or keyboard support now**. But two interface
decisions, made now, keep the door open at near-zero cost — and both are things
we're touching anyway in the rewrite:

1. **Generalize the actuation seam.** §5.2's `enum Backend { Ratbagd, Native }`
   is really a `trait Actuator` — "apply this profile to this device." Extend the
   mental model to `{ Ratbagd, Native(hidpp20), Vial(raw-HID), OpenTabletDriver(RPC),
   … }`. The onboard backends *write the device*; the software-driven backends
   *reconfigure a host daemon*. The product layer (focus/rules/GUI) calls the
   trait and never learns which.
2. **Make the profile model capability-based, not mouse-hardcoded.** Today a
   profile hardcodes mouse concepts (DPI stages, resolution slots). Model it
   instead as a bundle of *optional capabilities*:
   - **Shared:** `profiles`/slots (the switching primitive itself), `bindings`
     (button/key → action/macro/remap), `rgb`, `report_rate`.
   - **Sensitivity family (one abstract axis, different units):** mouse `dpi`,
     tablet `area`+`sensitivity`, gamepad `deadzone`/curve — unify as a
     parametric input-transfer capability.
   - **Class-specific:** tablet `pressure_curve` + `absolute/relative` mode;
     keyboard `layers`/`combos`; gamepad `gyro`/`rumble`.
   - **The decisive extra field: a per-capability `authority` flag —
     `onboard-persisted` vs `host-daemon-held`.** It determines whether we write
     the device or reconfigure a daemon, and whether the profile survives a reboot
     without gamerat running. Making this first-class (instead of assuming
     libratbag's onboard semantics everywhere) is the single thing that lets one
     model cover both halves of the ecosystem.

Concretely: DPI becomes *one instance* of the sensitivity capability; mouse
buttons become *one instance* of bindings. `gamerat-proto`'s `GameratProfile`
grows optional typed capability blocks + the authority flag; nothing else has to
change to *not preclude* tablets/keyboards later.

### 7c.4 Honest caveats

- **Tablets dent gamerat's "push it onto the hardware" identity.** A tablet
  profile is pure host state — the onboard premise doesn't apply. That's fine, but
  it reframes gamerat from "configure your mouse firmware" to "manage per-app
  peripheral config, onboard *or* host-driven." Worth deciding if that's the
  identity we want before advertising tablet support.
- **Priority, if we ever expand: keyboards (Vial) > tablets.** Keyboards are the
  higher-value, lower-friction expansion — same onboard model as mice, a real HID
  protocol, existing third-party clients — whereas tablets need a whole new
  software-driven adapter shape. The user asked for tablets specifically (a
  legitimate niche-but-adjacent gaming use case via osu!); the accommodating
  design above serves both, but if effort is ever spent, Vial is the cheaper win.
  (Vial wrinkle: a security-unlock step gamerat would have to handle.)
- **Scope-creep discipline.** The trap is generalizing the interfaces so hard, so
  early, that the mouse MVP slows down. The rule: design the *data model*
  (capabilities + authority flag) and the *one seam* (`trait Actuator`) to
  accommodate; build only mice now; add a backend when a concrete user need (and a
  tester) shows up. Accommodating ≠ building.

---

## 8. Risks & open questions

- **RE risk on multi-key macro encoding** — the whole thesis. Spike 0 gates it.
- **Testing without hardware breadth** — we can only meaningfully test hidpp20 on
  the G502. Everything else stays on the C fallback precisely because we can't
  test it. Be disciplined about not claiming coverage we can't verify.
- **`hidraw` access & udev** — need to nail the uaccess/devd decision early; it
  colors the whole native path.
- **Losing the C fallback too early** — the 15 drivers / 121 devices are real
  value. The plan must keep ratbagd reachable until per-device Rust parity is
  proven. The `enum Backend` design is specifically to protect this.
- **Two-backend maintenance burden during transition** — real but bounded, and
  the alternative (drop C, lose device coverage) is worse.
- **Scope discipline** — "rewrite libratbag" can balloon into "reimplement the
  Linux gaming-mouse ecosystem." The antidote is the hidpp20-first, G502-first,
  fallback-always framing. Resist generalizing a driver before there's hardware
  to test it on.

---

## 9. One-paragraph recommendation

Don't clone ratbagd's D-Bus surface, and don't attempt a full libratbag
replacement. Build a new **`gamerat-device`** Rust crate that owns *only* the
hidpp20 path first, reusing libratbag's `.device` data files verbatim, and slot
it behind the **existing `RatbagClient` API in `gamerat-ratbag` as a
runtime-selectable Native backend with the C stack retained as fallback** for
every device we don't yet support. Keep the device layer separate from
`gamerat-daemon` (the privilege boundary demands it) — as a library, optionally
hosted by a minimal `gamerat-devd`. Gate the entire effort on **Spike 0**:
proving we can write and read back a real multi-key macro on the G502. If that
spike succeeds, the rewrite pays for itself in the exact capability that
triggered this question; if it fails, we've spent days, not months, learning the
premise was wrong.
