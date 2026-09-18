# M1 — Hardware Profiler: Test Evidence

**Branch:** `feat/m1-hardware-profiler` · **Date:** 2026-09-10
**Not merged to `main`.**

---

## Summary

| Check | Result |
|---|---|
| `cargo test` | **47 passed, 0 failed** |
| `cargo clippy --all-targets -- -D warnings` | clean |
| `cargo fmt --check` | clean |
| `npm run lint` | 0 warnings, 0 errors |
| `npm run build` | ✅ 192 modules |
| Tier selection, all 5 tiers | ✅ verified by execution (below) |

**One real bug was caught by these tests and fixed** — see §3.

---

## 1. Why the forced-profile override exists

The dev laptop has 8 GB. Tiers T2/T3/T4 can therefore never be exercised on real
hardware there, so without simulation those code paths would ship unverified.

```bash
ORION_FORCE_PROFILE=T3          # shorthand: a typical machine of that tier
ORION_FORCE_PROFILE=ram=32,avail=6,cores=16,vram=12   # exact synthetic machine
```

Keys: `ram`, `avail`, `cores`, `vram`, `disk` (GiB). `ram` is required.
Simulated profiles are **always flagged in the UI and logs** so they can never be
mistaken for a real measurement — enforced by
`simulated_profiles_are_flagged_to_the_user`.

---

## 2. Executed evidence

Run against `examples/tierdemo`, real output:

| `ORION_FORCE_PROFILE` | RAM total / free | VRAM | Result |
|---|---|---|---|
| *(unset — this sandbox)* | 1.9 / 1.5 | — | **T0** + "too little for a local model" |
| `T1` | 8 / 6 | — | **T1** Minimal |
| `T2` | 16 / 12 | — | **T2** Standard |
| `T3` | 32 / 24 | 12 | **T3** Performance |
| `T4` | 64 / 48 | 24 | **T4** Workstation |
| `ram=16,cores=12` | 16 / 12 | — | **T2** |
| `ram=32,avail=6` | 32 / 6 | — | **T1** — *"Free memory currently limits this to T1, below the T3 this machine could otherwise run."* |
| `ram=64,vram=24` | 64 / 48 | 24 | **T4** |
| `ram=4,avail=3` | 4 / 3 | — | **T0** + warning |

The `ram=32,avail=6` row is the important one: a 32 GB workstation under memory
pressure is correctly held down to T1 **and told why**.

---

## 3. Bug found and fixed during testing

**Symptom.** A 16 GB machine was recommended **T3**, and a 32 GB machine **T4**.

**Cause.** Tier was chosen purely from *currently free* RAM. A 16 GB box with
12 GB free has 10 GB after headroom, which "fits" T3's 9 GB — so it claimed T3
and would have left the user's real work with nothing.

**Fix.** Two independent caps, and the lower wins:

- `by_class` — the machine's class from **installed** RAM (8→T1, 16→T2, 32→T3, 64+→T4)
- `by_free` — what genuinely fits **right now**; can only ever *lower* the result

This preserves "budget against available, not total" while stopping a
momentarily-idle machine from over-claiming. Covered by `sixteen_gb_gets_t2`,
`thirtytwo_gb_gets_t3`, and `budgets_available_ram_not_total`.

**This is exactly the class of bug the override was built to catch, on hardware
that cannot reproduce it.**

---

## 4. Test inventory (47)

**Profiler (22)** — tier selection per size; GPU lifting tier; available-vs-total
budgeting; OS headroom; T0 fallback; reasoning always present; CPU/disk warnings;
simulated flagging; spec parsing (shorthand, long form, defaults, overrides);
malformed-spec rejection; tier parse/step-down/ordering.

**Models (20)** — SHA-256 against three NIST vectors plus a 1 MB multi-block input;
file hashing; registry integrity (one model per tier, unique ids, RAM matches tier
budget); **licence gate — anything marked `redistributable` must be Apache/MIT**;
install-state detection (installed, missing, truncated→corrupt, `.part`→downloading);
fallback to best installed model; highest-tier preference; user-supplied `.gguf`
discovery; registry override file; invalid override falls back; checksum verify
pass/fail/absent.

**Database (5)** — carried from M0, including the Serina regression test.

---

## 5. Deliberate design choices

**Detect, don't ask.** The user is never made to self-report specs. Orion measures,
recommends, explains, and allows override.

**Reasoning is user-visible.** Every recommendation carries the sentences that
produced it, shown in the System panel — not hidden behind a spinner.

**Fallback over refusal.** `resolve_model` order: `ORION_MODEL_PATH` → the tier's
model → best installed of any tier → any user `.gguf`. A user who already has
weights is never blocked because our preferred file is missing.

**Licence is a data field, enforced by a test.** `redistributable: true` requires
Apache/MIT. Gemma and Llama carry custom terms and must never be flagged
redistributable. This is what keeps the future offline bundle legal.

**SHA-256 implemented in-tree** (~60 lines, no `unsafe`) rather than adding a
dependency to a security-sensitive path. Validated against NIST vectors.

---

## 6. Not yet verified

- **In-app download with progress UI.** Models install via `scripts/fetch-model.sh`;
  the manager reports state but does not yet fetch. Next increment.
- **Post-load tok/s benchmark** and automatic step-down — needs real weights.
- **Runtime GUI verification.** No display server, GTK/WebKit, or weights in this
  environment. Compile- and logic-verified only; first real launch is on your laptop.
- **`nvidia-smi` GPU detection** — no GPU here to test against.
