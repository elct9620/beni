# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

beni is an mruby toolchain monorepo: a Ruby gem (`beni`) vendors mruby + wasi-sdk and builds the mruby archive through Rake, and two Rust crates (`beni-sys` bindgen FFI, `beni` typed wrapper) bind the resulting archive — the magnus / rb-sys split applied at the mruby boundary — with `beni-macros` carrying the `beni` crate's proc macros as magnus-macros does magnus's. The unpublished `beni-tests` crate holds the typed suite in consumer position. wasm32-wasip1 is a downstream verification target only (for kobako), not a product target. All four published packages release in lockstep under one version.

## Principles

Each section below is one principle, and an earlier one overrides a later one on conflict.

| Order | Principle | Decides |
|---|---|---|
| 1 | Spec Authority | what the code must do |
| 2 | Upstream Baseline | whose shape a package copies |
| 3 | Magnus Naming | what a `beni` item is called |
| 4 | Proven Divergence | when `beni` departs from magnus |
| 5 | Defensive Layers | what is not modelled |
| 6 | Prohibitions | what never lands |

### Spec Authority

SPEC.md is the source of truth, and authority flows from spec to code.

```
SPEC.md ──defines──▶ code     mismatch: implementation bug
   ▲                          silent:   extend SPEC first
   └── never edited to ratify the code
```

The spec runs ahead of the code, so an unimplemented behavior is the roadmap. A cross-package contract is defined once at its write end, with its constants in SPEC's Terminology; cite those terms instead of restating them. The typed surface's safety bar is SPEC's "Graduation, safety, and coverage".

### Upstream Baseline

Each package copies its upstream. Where a shape came from is never a reason to keep it: kobako-derived code is scaffolding, and mruby's C name is already mapped by `.api_coverage.yml`.

| Package | Upstream |
|---|---|
| gem build | mruby's `rake`, `MRUBY_CONFIG`, untouched `default.rb` |
| gem toolchain | wasi-sdk's `/opt/wasi-sdk` |
| `beni-sys` | `-sys` crates: `*_LIB_DIR`, `links =` |
| `beni`, `beni-macros` | the latest stable magnus |

For `beni`, magnus decides the name, signature, receiver, carrying trait, and whether an operation is `unsafe`. Read magnus from its latest release's source, and mruby from `vendor/mruby`, before a claim reaches SPEC or a comment. Growing `beni` toward magnus's surface is the product goal.

### Magnus Naming

Names follow magnus's C function index (`src/lib.rs`), and a function it leaves unnamed takes the same rules.

| C function | Rust name |
|---|---|
| a type conversion | `TryConvert` |
| `rb_` and receiver type prefix | dropped: `ivar_get`, `new_instance` |
| a predicate | `is_x` |
| a handle-returning read | `to_r_*` |
| an operation a Rust trait names | that trait's impl |

`TryConvert` is magnus's implicit conversion. A coercion with no implicit protocol, such as `mrb_obj_to_sym`, stays a method.

### Proven Divergence

`beni` departs from magnus only on an mruby ↔ CRuby difference proven in source. It reaches only as far as magnus's path stops working; a path that still works with one more step is a cost, not a divergence.

| mruby against CRuby | `beni`'s shape | Instance |
|---|---|---|
| narrower | encode the limit in the type | `Object` only where `obj_iv_p` holds |
| lacks magnus's reason | drop what it forced | `value::qnil()` takes no `Mrb` |
| has more | give it magnus's shape | the `RCptr` handle |

The divergent item's doc comment names the evidence.

### Defensive Layers

Model what SPEC requires and no layer against a problem that does not exist. "No consumer needs it yet" never rejects growth toward magnus.

| Rejected layer |
|---|
| `Bundler.with_unbundled_env` isolation |
| a minirake fallback |
| a cross-compile abstraction beyond wasm32 |

### Prohibitions

Hooks and CI run the tooling gates; these are the rules no gate checks.

| Area | Never |
|---|---|
| ABI | hard-code an ABI define in a crate |
| ABI | let an archive without its sidecar fall back to guessing |
| Config | ship a config template in the gem |
| Lint | widen `.rubocop.yml` exclusions or add `#[allow]` |
| Docs | narrate mechanism, incidents, or rejected suggestions |
| Docs | use YARD tags or rustdoc intra-doc links |
| Tests | fake the task layer for consumer-visible behavior |
| Tests | test in `crates/beni` what public paths reach |
| Tests | sweep `test/scenarios/` into the default glob |
| RBS | patch `sig/patches/` before trying a `Steepfile` library |
| Deps | commit a dependency change without its lock file |
| Notes | put a non-permanent design note outside `tmp/` |

A tool-vs-tool conflict is the one allowed lint widening: `Style/DataInheritance` is off because ruby/rbs documents `class X < Data.define(...)`.

## Build Pipeline

The repo dogfoods its own gem: the Rakefile wires `Beni::Tasks` with the validation config `build_config/mruby.rb`. That config builds host and wasi, ABI-pinned with `MRB_INT32` and `MRB_WORDBOX_NO_INLINE_FLOAT`; the gem's default stays mruby's untouched upstream config. `rake rust:verify` is the single local gate.

| Task | Command |
|------|---------|
| Default CI task (test + rubocop + steep) | `bundle exec rake` |
| Run all Ruby tests | `bundle exec rake test` |
| Run one Ruby test file | `bundle exec ruby -Ilib -Itest test/beni/test_builder.rb` |
| Run one Ruby test by name | `bundle exec ruby -Ilib -Itest test/beni/test_builder.rb -n /pattern/` |
| Steep type check only | `bundle exec rake steep` |
| Full Rust verification chain | `bundle exec rake rust:verify` |
| Build vendored mruby (both targets) | `bundle exec rake beni:build` |
| Point the chain at another mruby release | `BENI_MRUBY_VERSION=4.1.0-rc2 bundle exec rake beni:build` |
| Stage toolchains only | `bundle exec rake beni:vendor:setup` |
| Remove build trees / toolchains / everything | `rake beni:clean` / `beni:vendor:clean` / `beni:vendor:clobber` |
| Run a consumer scenario | `cd test/scenarios/default_host && rake scenario:setup beni:build scenario:verify` |
| Interactive console | `bin/console` |

## Layering

### The packages around the staged archive

```
beni gem (lib/)                          crates/
─────────────────────────────────       ─────────────────────────────────────
Tasks      Beni::Tasks (Rake::TaskLib    beni      typed wrapper (magnus idioms)
  │          — beni:* task surface)        L2  convert::{IntoValue, FromValue} · TryConvert
Build      Beni::Builder (drives            │   scan_args (magnus-shaped frame reads)
  │          mruby's own rake via            │   state::protect (mrb_protect_error)
  │          MRUBY_CONFIG; requests          L1  Mrb RAII · Value + typed
  │          flags.mak file tasks)           │   newtypes · Ccontext †
Config     Beni::BuildConfig                 L0  sys (beni_sys::* + helpers)
  │          (beni:config: copies the      ──────────────────────────────
  │          staged upstream default)
Vendor     Beni::Vendor façade →          beni-sys  bindgen FFI surface
             Vendor::{Toolchain,            build.rs: archive discovery ·
             Downloader, Checksum,          flags.mak parse · bindgen +
             Tarball}                       wrap_static_fns (single C TU) ·
                                            links = "mruby" (one linker)

                                          beni-macros  wrap / TypedData /
                                            InlineStruct derive,
                                            re-exported by beni

                                          beni-tests  publish = false; the
                                            typed suite run from consumer
                                            position, always against a real
                                            archive (outside default-members)
        │                                          ▲
        └── stages vendor/mruby/build/<name>/lib/ ─┘
            libmruby.flags.mak + the archive it names (the staged
            path; the sidecar is the sole ABI alignment channel)
```

- **† carried by a capability feature.** A capability mruby keeps in a gem rather than its core sits behind a cargo feature on the `beni` crate — `compiler` (`Ccontext` and `Mrb::load_string`) is the first, on by default. Declared, never probed: the crate reads no gem inventory, so an item is gated by what a consumer asked for and not by what the archive happens to carry. A feature carries operations and never the shapes their results are reported in, which is why `ParseMessage` and `Error::Syntax` stay ungated — `Error` has one shape in every build. The drift gate reads the same axis: `api:surface` expects a gated item in the net body gated on its feature. A dependency feature is the second axis beside it: conversions to a third-party crate's types, off by default and gated as magnus gates the same integration — `bytes` (`TryConvert` / `IntoValue` for `bytes::Bytes`) is the first.
- **The gem never reads the build config** — the config owns target definitions; beni only verifies that declared targets produced their artifacts.
- **`Beni::Vendor::Toolchain` is a declarative `Data` value** exposing the fetch → verify → install pipeline; `Beni::Tasks` loops it into `file`/`task` declarations. A new tarball-based toolchain is one factory method in `Beni::Vendor`.
- **`beni-sys/build.rs` is the only consumer of `MRUBY_LIB_DIR` / `WASI_SDK_PATH`** — libclang and discovery logic stay a sys-only build concern.
- **Widths and the release are read, not declared.** They are ABI facts, not capabilities: `beni-sys` publishes each from its bindings as `links` metadata, and `beni/build.rs` turns them into cfgs. `mrb_int64` and `mrb_float32` admit a Rust number into a `Value` only where every value it holds fits. The rb-sys-shaped `mruby_{lt,lte,eq,gte,gt}_X_Y` gate release-dependent behavior; a C-side difference stays in `wrapper.h` behind `MRUBY_RELEASE_NO`. Width-dependent conversions stay lint-clean under both widths by spelling the target as `sys::mrb_int` (or an identity per width) — clippy flags a conversion only when a concrete target type equals its source.
- The typed `mrb_func_t` at the `beni` crate root uses `Value` slots; `Class::define_method` transmutes it once to the raw `sys::mrb_func_t` — ABI-identical because `Value` is `#[repr(transparent)]` over `mrb_value`.

## Entry Points

Each entry point's own header or module doc is the authority for its area; read it before the code below it.

| Topic | Entry point | Note |
|-------|-------------|------|
| Behavior contracts | `SPEC.md` | features, error table, Terminology |
| Task surface | `lib/beni/tasks.rb` | DSL in `lib/beni/dsl/` |
| Vendor pipeline | `lib/beni/vendor.rb` | built-in pairs, platforms, factory registry |
| mruby build driving | `lib/beni/builder.rb` | archive and sidecar per target |
| Config generation | `lib/beni/build_config.rb` | copies the staged upstream default |
| ABI alignment | `crates/beni-sys/build.rs` | file-top comment is the contract |
| Typed wrapper | `crates/beni/src/lib.rs` | module doc maps tiers L0–L2 |
| Wrapper macros | `crates/beni-macros/src/` | tested on `beni`'s re-exports |
| Wrapper tests | `crates/beni-tests/src/` | run by `rake rust:test` only |
| Consumer scenarios | `test/scenarios/*/Rakefile` | headers list covered postures |
| Verification chain | `tasks/rust.rake` | header explains each leg |
| Documentation bindings | `tasks/docs.rake` | generated, never tracked |
| CI lanes | `.github/workflows/main.yml` | rationale inline |
| RBS signatures | `sig/beni/` | mirrors `lib/beni/` 1:1 |
| API coverage | `.api_coverage.yml` | rendered to `docs/api_coverage.md` |

The wrapper tests sit in consumer position, reaching public paths only against a staged archive. `surface_test.rs` names every inherent pub fn and re-exported macro, and `api:surface` keeps that list in step with the crate. In the coverage manifest, a symbol absent from every section is API still owed. `rake api:priority` ranks what is owed by downstream use, and `BENI_CONSUMER_PATHS` points it at a consumer checkout.
