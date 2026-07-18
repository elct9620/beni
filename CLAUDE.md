# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

beni is an mruby toolchain monorepo: a Ruby gem (`beni`) vendors mruby + wasi-sdk and builds `libmruby.a` through Rake, and two Rust crates (`beni-sys` bindgen FFI, `beni` typed wrapper) bind the resulting archive — the magnus / rb-sys split applied at the mruby boundary. wasm32-wasip1 is a downstream verification target only (for kobako), not a product target. All three packages release in lockstep under one version.

## Principles

Apply these in order — earlier principles override later ones on conflict.

1. **SPEC.md is the source of truth, and authority flows spec → code.** The spec is deliberately ahead of the implementation; unimplemented spec behaviors are the roadmap, and a spec/code mismatch is an implementation bug. Never edit SPEC.md to ratify what the code happens to do — when SPEC is silent, extend it first, then implement. Cross-package contracts (archive discovery, compile-flags sidecar, staged path, linked signal) are defined once at the write end with constants in SPEC's Terminology; cite those terms instead of restating them.

2. **kobako-derived code is scaffolding, not precedent.** Much of this repo was extracted from kobako; matching kobako's shape is never a design justification. Follow upstream conventions instead — mruby's own (`rake` entry point, `MRUBY_CONFIG`, untouched `build_config/default.rb` as the gem default), wasi-sdk's (`/opt/wasi-sdk`), the `-sys` crate conventions (`*_LIB_DIR`, `links =` metadata), and magnus's wrapper idioms for the `beni` crate's API surface.

3. **Verify toolchain facts against vendored sources, not memory.** mruby behavior claims must be checked in `vendor/mruby` before being relied on or written into SPEC/comments — e.g. `MRuby::Lockfile` is enabled at autoload (the class body calls `enable`), not opt-in.

4. **The compile-flags sidecar is the only ABI alignment channel.** `beni-sys` parses `libmruby.flags.mak` next to each archive; never hard-code ABI defines in the crates, and never let a staged archive without its sidecar fall back to guessing. The gem ships no config template — `beni:config` copies the configured version's upstream default config from the staged mruby source; `build_config/mruby.rb` is the repo's own validation config (the generate-then-edit consumer posture, kept committed).

5. **Follow language community conventions via tooling.** Ruby: Rubocop + Steep; Rust: `cargo fmt` + `cargo clippy -D warnings` (also under `--target wasm32-wasip1` when the wasi archive is staged) + `cargo doc -D warnings --document-private-items`. All run via PostToolUse/Stop hooks and block on failure. When a cop or lint fires, shrink the code to fit the tool — don't widen `.rubocop.yml` exclusions or add `#[allow]`. Tool-vs-tool conflicts are the one justified widening: `Style/DataInheritance` is disabled because ruby/rbs documents `class X < Data.define(...)` as the Steep-friendly form.

6. **Don't pre-abstract; model exactly what SPEC requires** — no defensive layers against problems that don't exist (rejected: `Bundler.with_unbundled_env` isolation, a minirake fallback, a generalized cross-compile abstraction beyond wasm32). Growing the `beni` crate toward magnus's API surface is still the product goal (SPEC-first per Principle 1); "no consumer needs it yet" never rejects that work.

7. **Docs and comments state intent in 1–2 sentences; don't narrate mechanism, incidents, or rejected suggestions** — code doesn't explain itself against problems it doesn't have. Ruby: RDoc prose (`+code+`, no YARD tags). Rust: backtick code spans, no rustdoc intra-doc links (they rot on renames and the `cargo doc` gate rejects breakage).

8. **`test/` holds unit tests; `test/scenarios/` holds consumer harnesses** — each a consumer-shaped Rakefile run through the gem's task surface alone (`scenario:setup` → `beni:build` → `scenario:verify`), excluded from the default glob (else the vendored mruby tree's own `*_test.rb` get swept in). New consumer-visible behavior gets a scenario, not a unit test that fakes the task layer.

9. **RBS mirrors `lib/` 1:1 under `sig/`.** The steep hook blocks Ruby edits without matching signatures. Missing stdlib sigs: reach for `library "<name>"` in `Steepfile` first, hand-rolled patches in `sig/patches/` last.

10. **Commit lock files** (`Cargo.lock`, `Gemfile.lock`, `rbs_collection.lock.yaml`) alongside the dependency changes that produced them. Non-permanent design notes go to `tmp/` (gitignored), never `docs/`.

11. **The typed `beni` surface graduates only what is safe to use without VM-internal reasoning** — a stronger bar than "cannot cause UB". An operation reaches the safe surface only when the wrapper can encode its invariant (a lifetime, carrier, or runtime check); otherwise the honest form is `unsafe` — a typed `unsafe fn` on the `beni` surface when a typed shape can still carry the value with one caller-owned invariant unencoded, or a raw `beni::sys` binding when the value is VM-internal with no shape to add (call-frame indices, `mrb_context` / RBreak fields). One unsafe only for want of an unbuilt carrier graduates once built; a permanently VM-internal one stays in `sys`. Zeroing a consumer's `sys::` use is never the goal. Refines Principle 6; the typed-surface contract lives in SPEC (e.g. `Proc::call` / `as_break` graduated, the raw frame-index shims stayed in `sys`). Every graduation is recorded in the `.api_coverage.yml` manifest — add the C symbol with the public Rust item that binds it (a Rust-native construct with no 1:1 C API goes under `extensions:` instead); `rake api:coverage` regenerates `docs/api_coverage.md` from it. Only record symbols reachable through a public interface: test-only or internal-only bindings stay out until they are genuinely graduated.

## Build Pipeline

The repo dogfoods its own gem: the Rakefile wires `Beni::Tasks` with the validation config `build_config/mruby.rb` (host + wasi targets, ABI-pinned with `MRB_INT32` + `MRB_WORDBOX_NO_INLINE_FLOAT`), while the gem's default stays mruby's untouched upstream config. `rake rust:verify` is the single local gate: `beni:build` → host `cargo check`/`test` → wasm32 `cargo check` → `rust:test:default` (tests against an upstream-default mruby build, catching int-width coincidences the MRB_INT32 config masks).

`beni-sys/build.rs` has three modes: real archive linked (`mruby_linked` cfg, published downstream via `DEP_MRUBY_LINKED`), host placeholder (no archive — `cargo check` passes, no FFI surface), wasm32 without staged toolchain (panics; never a placeholder).

CI (`.github/workflows/main.yml`) runs four lanes: **test** (Ruby matrix, default task), **lint** (Rust fmt/clippy/doc in placeholder mode), **verify** (3 OS full `rust:verify` + linked clippy), **scenario** (consumer harnesses). Tarballs are deliberately not cached — the download path is itself under test.

## Common Commands

| Task | Command |
|------|---------|
| Default CI task (test + rubocop + steep) | `bundle exec rake` |
| Run all Ruby tests | `bundle exec rake test` |
| Run one Ruby test file | `bundle exec ruby -Ilib -Itest test/beni/test_builder.rb` |
| Run one Ruby test by name | `bundle exec ruby -Ilib -Itest test/beni/test_builder.rb -n /pattern/` |
| Steep type check only | `bundle exec rake steep` |
| Full Rust verification chain | `bundle exec rake rust:verify` |
| Build vendored mruby (both targets) | `bundle exec rake beni:build` |
| Stage toolchains only | `bundle exec rake beni:vendor:setup` |
| Remove build trees / unpacked toolchains / everything | `rake beni:clean` / `rake beni:vendor:clean` / `rake beni:vendor:clobber` |
| Run a consumer scenario | `cd test/scenarios/default_host && rake scenario:setup beni:build scenario:verify` |
| Interactive console | `bin/console` |

## Layering

### Three packages around the staged archive

```
beni gem (lib/)                          crates/
─────────────────────────────────       ─────────────────────────────────────
Tasks      Beni::Tasks (Rake::TaskLib    beni      typed wrapper (magnus idioms)
  │          — beni:* task surface)        L2  convert::{IntoValue, FromValue}
Build      Beni::Builder (drives            │   state::args (Format dispatch)
  │          mruby's own rake via            │   state::protect (mrb_protect_error)
  │          MRUBY_CONFIG; requests          L1  Mrb RAII · Value + typed
  │          flags.mak file tasks)           │   newtypes · Ccontext
Config     Beni::BuildConfig                 L0  pub use beni_sys as sys
  │          (beni:config: copies the      ──────────────────────────────
  │          staged upstream default)
Vendor     Beni::Vendor façade →          beni-sys  bindgen FFI surface
             Vendor::{Toolchain,            build.rs: archive discovery ·
             Downloader, Checksum,          flags.mak parse · bindgen +
             Tarball}                       wrap_static_fns (single C TU) ·
                                            links = "mruby" → DEP_MRUBY_LINKED
        │                                          ▲
        └── stages vendor/mruby/build/<name>/lib/ ─┘
            libmruby.a + libmruby.flags.mak (the staged path;
            the sidecar is the sole ABI alignment channel)
```

- **The gem never reads the build config** — the config owns target definitions; beni only verifies that declared targets produced their artifacts.
- **`Beni::Vendor::Toolchain` is a declarative `Data` value** exposing the fetch → verify → install pipeline; `Beni::Tasks` loops it into `file`/`task` declarations. A new tarball-based toolchain is one factory method in `Beni::Vendor`.
- **`beni-sys/build.rs` is the only consumer of `MRUBY_LIB_DIR` / `WASI_SDK_PATH`** — libclang and discovery logic stay a sys-only build concern.
- The typed `mrb_func_t` at the `beni` crate root uses `Value` slots; `Class::define_method` transmutes it once to the raw `sys::mrb_func_t` — ABI-identical because `Value` is `#[repr(transparent)]` over `mrb_value`.

## Where to Look

| Topic | Entry points | Notes |
|-------|--------------|-------|
| Behavior contracts | `SPEC.md` | Single file: Features per package, exhaustive error table, Terminology constants. Check here before reading code. |
| Task surface / settings | `lib/beni/tasks.rb` | Consumes the resolved `Beni::Configuration`; the declarative DSL itself lives in `lib/beni/dsl/` (`DSL::Context` and friends). |
| Vendor pipeline | `lib/beni/vendor.rb` (façade) | Pinned versions, platform detection, factory registry; pipeline stages in `lib/beni/vendor/`. |
| mruby build driving | `lib/beni/builder.rb` | Spawns mruby's own rake; artifact = archive + sidecar per target. |
| Config generation | `lib/beni/build_config.rb` | Copies the staged upstream default (see Principle 4); `build_config/mruby.rb` is the repo's own validation config. |
| Archive discovery / ABI alignment | `crates/beni-sys/build.rs` | The file-top comment is the authoritative mode/contract description. |
| Typed wrapper | `crates/beni/src/lib.rs` | Module-level doc carries the L0–L2 tier map. |
| Consumer scenarios | `test/scenarios/*/Rakefile` | Each documents the consumer path it pins; harness contract is `scenario:setup` → `beni:build` → `scenario:verify`. |
| CI lanes | `.github/workflows/main.yml` | Lane rationale is commented inline (e.g. why wasm clippy lives in verify, not lint). |
| RBS signatures | `sig/beni/` | Mirrors `lib/beni/` 1:1; stdlib via `Steepfile`, patches in `sig/patches/`. |
| API coverage | `.api_coverage.yml` → `docs/api_coverage.md` | `rake api:coverage` diffs mruby's scanned C surface against the Rust layers; sys tier auto-derived, typed tier hand-curated in the manifest (see Principle 11). |
