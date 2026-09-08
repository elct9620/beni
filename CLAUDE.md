# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

beni is an mruby toolchain monorepo: a Ruby gem (`beni`) vendors mruby + wasi-sdk and builds `libmruby.a` through Rake, and two Rust crates (`beni-sys` bindgen FFI, `beni` typed wrapper) bind the resulting archive — the magnus / rb-sys split applied at the mruby boundary. The unpublished `beni-tests` crate holds the typed suite in consumer position. wasm32-wasip1 is a downstream verification target only (for kobako), not a product target. All three published packages release in lockstep under one version.

## Principles

Apply these in order — earlier principles override later ones on conflict.

1. **SPEC.md is the source of truth, and authority flows spec → code.** The spec is deliberately ahead of the implementation; unimplemented spec behaviors are the roadmap, and a spec/code mismatch is an implementation bug. Never edit SPEC.md to ratify what the code happens to do — when SPEC is silent, extend it first, then implement. Cross-package contracts (archive discovery, compile-flags sidecar, staged path, documentation bindings) are defined once at the write end with constants in SPEC's Terminology; cite those terms instead of restating them.

2. **kobako-derived code is scaffolding, not precedent.** Much of this repo was extracted from kobako; matching kobako's shape is never a design justification. Follow upstream conventions instead — mruby's own (`rake` entry point, `MRUBY_CONFIG`, untouched `build_config/default.rb` as the gem default), wasi-sdk's (`/opt/wasi-sdk`), the `-sys` crate conventions (`*_LIB_DIR`, `links =` metadata), and magnus's wrapper idioms for the `beni` crate's API surface.

3. **Verify toolchain facts against vendored sources, not memory.** mruby behavior claims must be checked in `vendor/mruby` before being relied on or written into SPEC/comments — e.g. `MRuby::Lockfile` is enabled at autoload (the class body calls `enable`), not opt-in.

4. **The compile-flags sidecar is the only ABI alignment channel.** `beni-sys` parses `libmruby.flags.mak` next to each archive; never hard-code ABI defines in the crates, and never let a staged archive without its sidecar fall back to guessing. The gem ships no config template — `beni:config` copies the configured version's upstream default config from the staged mruby source; `build_config/mruby.rb` is the repo's own validation config (the generate-then-edit consumer posture, kept committed).

5. **Follow language community conventions via tooling.** Ruby: Rubocop + Steep; Rust: `cargo fmt` + `cargo clippy -D warnings` (also under `--target wasm32-wasip1` when the wasi archive is staged) + `cargo doc -D warnings --document-private-items`. All run via PostToolUse/Stop hooks and block on failure. When a cop or lint fires, shrink the code to fit the tool — don't widen `.rubocop.yml` exclusions or add `#[allow]`. Tool-vs-tool conflicts are the one justified widening: `Style/DataInheritance` is disabled because ruby/rbs documents `class X < Data.define(...)` as the Steep-friendly form.

6. **Don't pre-abstract; model exactly what SPEC requires** — no defensive layers against problems that don't exist (rejected: `Bundler.with_unbundled_env` isolation, a minirake fallback, a generalized cross-compile abstraction beyond wasm32). Growing the `beni` crate toward magnus's API surface is still the product goal (SPEC-first per Principle 1); "no consumer needs it yet" never rejects that work.

7. **Docs and comments state intent in 1–2 sentences; don't narrate mechanism, incidents, or rejected suggestions** — code doesn't explain itself against problems it doesn't have. Ruby: RDoc prose (`+code+`, no YARD tags). Rust: backtick code spans, no rustdoc intra-doc links (they rot on renames and the `cargo doc` gate rejects breakage).

8. **`test/` holds unit tests; `test/scenarios/` holds consumer harnesses** — each a consumer-shaped Rakefile run through the gem's task surface alone (`scenario:setup` → `beni:build` → `scenario:verify`), excluded from the default glob (else the vendored mruby tree's own `*_test.rb` get swept in). New consumer-visible behavior gets a scenario, not a unit test that fakes the task layer. The Rust side splits the same way: `crates/beni-tests/` holds the wrapper's behavior suite, reaching it through public paths alone and always against a staged archive; a `#[cfg(test)]` module inside `crates/beni/` holds only what no consumer can observe (a private field, a `pub(crate)` helper). A new wrapper test goes to `beni-tests` unless it needs something private.

9. **RBS mirrors `lib/` 1:1 under `sig/`.** The steep hook blocks Ruby edits without matching signatures. Missing stdlib sigs: reach for `library "<name>"` in `Steepfile` first, hand-rolled patches in `sig/patches/` last.

10. **Commit lock files** (`Cargo.lock`, `Gemfile.lock`, `rbs_collection.lock.yaml`) alongside the dependency changes that produced them. Non-permanent design notes go to `tmp/` (gitignored), never `docs/`.

11. **The typed `beni` surface graduates only what is safe to use without VM-internal reasoning** — a stronger bar than "cannot cause UB". An operation reaches the safe surface only when the wrapper can encode its invariant (a lifetime, carrier, or runtime check); otherwise the honest form is `unsafe` — a typed `unsafe fn` when a typed shape can still carry the value with one caller-owned invariant unencoded, or a raw `beni::sys` binding when the value is VM-internal with no shape to add, where a safe-looking wrapper would misrepresent its sharpness. One unsafe only for want of an unbuilt carrier graduates once built; a permanently VM-internal one stays in `sys`, and zeroing a consumer's `sys::` use is never the goal. Refines Principle 6; the contract itself lives in SPEC. Every graduation is recorded in `.api_coverage.yml`, whose header defines the sections — what matters here is the rule they exist to keep: a capability merely awaiting a carrier is recorded in none of them, which is what makes an unrecorded symbol mean "still owed" and nothing else. A symbol from a header mruby marks internal to the library is admitted only for the typed item that carries it, by copying its one declaration into `wrapper.h` rather than including that header.

## Build Pipeline

The repo dogfoods its own gem: the Rakefile wires `Beni::Tasks` with the validation config
`build_config/mruby.rb` (host + wasi, ABI-pinned with `MRB_INT32` + `MRB_WORDBOX_NO_INLINE_FLOAT`),
while the gem's default stays mruby's untouched upstream config. `rake rust:verify` is the single
local gate; each leg says what it is for in `tasks/rust.rake`'s header, and each CI lane in
`.github/workflows/main.yml`'s inline comments.

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

### The packages around the staged archive

```
beni gem (lib/)                          crates/
─────────────────────────────────       ─────────────────────────────────────
Tasks      Beni::Tasks (Rake::TaskLib    beni      typed wrapper (magnus idioms)
  │          — beni:* task surface)        L2  convert::{IntoValue, FromValue}
Build      Beni::Builder (drives            │   state::args (Format dispatch)
  │          mruby's own rake via            │   state::protect (mrb_protect_error)
  │          MRUBY_CONFIG; requests          L1  Mrb RAII · Value + typed
  │          flags.mak file tasks)           │   newtypes · Ccontext †
Config     Beni::BuildConfig                 L0  pub use beni_sys as sys
  │          (beni:config: copies the      ──────────────────────────────
  │          staged upstream default)
Vendor     Beni::Vendor façade →          beni-sys  bindgen FFI surface
             Vendor::{Toolchain,            build.rs: archive discovery ·
             Downloader, Checksum,          flags.mak parse · bindgen +
             Tarball}                       wrap_static_fns (single C TU) ·
                                            links = "mruby" (one linker)

                                          beni-tests  publish = false; the
                                            typed suite run from consumer
                                            position, always against a real
                                            archive (outside default-members)
        │                                          ▲
        └── stages vendor/mruby/build/<name>/lib/ ─┘
            libmruby.a + libmruby.flags.mak (the staged path;
            the sidecar is the sole ABI alignment channel)
```

- **† carried by a capability feature.** A capability mruby keeps in a gem rather than its core sits behind a cargo feature on the `beni` crate — `compiler` (`Ccontext` and `Mrb::load_string`) is the first, on by default. Declared, never probed: the crate reads no gem inventory, so an item is gated by what a consumer asked for and not by what the archive happens to carry. A feature carries operations and never the shapes their results are reported in, which is why `ParseMessage` and `Error::Syntax` stay ungated — `Error` has one shape in every build. The drift gate reads the same axis: `api:surface` expects a gated item in the net body gated on its feature.
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
| Typed wrapper's tests | `crates/beni-tests/src/*_test.rs` | Consumer position: public paths only, always against a staged archive. `surface_test.rs` names every inherent pub fn from outside, so a dropped re-export breaks it; `api:surface` keeps that list and the crate's surface in step. Reached by `rake rust:test`, not by a bare `cargo test`. |
| Consumer scenarios | `test/scenarios/*/Rakefile` | Each documents the consumer path it pins; harness contract is `scenario:setup` → `beni:build` → `scenario:verify`. Read the headers to see which postures are already covered before adding one. |
| Verification chain | `tasks/rust.rake`, `tasks/docs.rake` | Header lists every leg of `rust:verify` and what it is for; the documentation bindings are generated rather than tracked, so `docs.rake` is where that contract lives. |
| CI lanes | `.github/workflows/main.yml` | Lane rationale is commented inline (e.g. why wasm clippy lives in verify, not lint). |
| RBS signatures | `sig/beni/` | Mirrors `lib/beni/` 1:1; stdlib via `Steepfile`, patches in `sig/patches/`. |
| API coverage | `.api_coverage.yml` → `docs/api_coverage.md` | `rake api:coverage` diffs mruby's scanned C surface against the Rust layers. The sys tier and `#define` equivalences are derived; the manifest hand-curates four per-symbol states plus `admitted:` beside them, so a symbol absent from all of them is API still owed and nothing else (see Principle 11). `rake api:priority` orders what is owed by what downstream calls — a Rust consumer's use ranks above any number of mrbgem uses, and `BENI_CONSUMER_PATHS` points the scan at a consumer checkout. |
