# Beni Specification

## Purpose

Beni gives Rust developers a magnus-like experience for mruby: a Ruby gem
manages the mruby build chain, and Rust crates expose a safe, typed API over
the resulting `libmruby.a`.

## Users

- Rust developers who embed mruby and want typed, memory-safe APIs instead of
  raw FFI.
- Rakefile-based projects that need a reproducible `libmruby.a` build wired
  into their own build pipeline.

## Impacts

- A Rust project can depend on the `beni` crate and call mruby without
  writing or maintaining FFI declarations by hand.
- A Rust project can produce `libmruby.a` via `rake beni:build` without
  vendoring mruby source or scripting tarball downloads.
- The same `version`, `build_config`, and `toolchains` inputs always build
  the same way: the same toolchain versions, compile flags, and staged
  layout.
- In a host build with no archive discovery variable set, a crate that
  depends on `beni` compiles in placeholder mode, so `beni` is safe to
  take as a transitive dependency in builds that do not opt into mruby.

## Success criteria

- A fresh checkout running `rake beni:build` produces `libmruby.a` and its
  compile-flags sidecar at the staged path for every declared target.
- A Rust binary built with `BENI_VENDOR_DIR` pointing at that vendor tree
  links the archive and runs an mruby interpreter through `Mrb::open`.
- `cargo check` on the `beni` crate succeeds with no archive discovery
  variable set, and `Mrb::open` returns an error.
- A `wasm32-wasip1` cross-build succeeds when `toolchains` includes
  `wasi-sdk`, the build config declares a target cross-compiled for
  wasm32, and `MRUBY_LIB_DIR` names that target's staged path.

## Non-goals

- Not a WebAssembly project — wasm32-wasip1 is a downstream verification
  target only.
- The gem does not embed mruby into Ruby programs; it only manages the
  toolchain for Rust consumers.
- No CRuby extension support — magnus and rb-sys own that boundary.

## Packages

One repository; the gem and both crates release in lockstep under a single
version number.

| Package | Registry | Responsibility |
|---|---|---|
| `beni` gem | rubygems.org | Rake tasks + DSL config that download mruby and build `libmruby.a` for the crates to consume |
| `beni-sys` crate | crates.io | `-sys` style FFI surface over the mruby C API, generated against the staged archive per supported mruby version |
| `beni` crate | crates.io | safe typed wrapper over `beni-sys`, aligned with magnus idioms |

Responsibility boundary: the gem stages toolchains and archives; `beni-sys`
binds them; the `beni` crate is the only package consumers write Rust against.

## Features

### beni gem — toolchain management

Consumers install the task library in their Rakefile:

```ruby
require "beni/tasks"

Beni::Tasks.new do |t|
  # override settings here; defaults below
end
```

| Setting | Type | Default |
|---|---|---|
| `vendor_dir` | directory path — where toolchains unpack and mruby builds | `vendor/` under the Rakefile's working directory. `BENI_VENDOR_DIR` env var overrides the default; an explicit DSL assignment overrides the env var. |
| `version` | mruby release version to download | `"4.0.0"` |
| `build_config` | mruby build-config file path (relative paths resolve against the Rakefile's working directory), or `nil` for mruby's upstream default | `nil` |
| `targets` | array of build-target names, matching the `MRuby::Build.new(<name>)` names in the config; a build defined without a name is named `host` by mruby | `["host"]` |
| `toolchains` | array of toolchain names to vendor, from `mruby` and `wasi-sdk` | `["mruby"]` |

| Task | Outcome |
|---|---|
| `beni:build` | toolchains staged, `libmruby.a` built per target |
| `beni:clean` | mruby build trees removed, vendored source kept |
| `beni:config` | self-contained, editable build config generated at the `build_config` path |
| `beni:vendor:setup` | configured toolchains downloaded and unpacked |
| `beni:vendor:clean` | unpacked toolchains removed, tarball cache kept |
| `beni:vendor:clobber` | vendor tree removed entirely, tarball cache included |

Behaviors:

- A clean build with no `build_config` uses mruby's untouched upstream
  default config.
- The vendor tree converges on each toolchain's selected version: a staged
  toolchain at any other version is replaced by `beni:vendor:setup`, and
  `beni:build` rebuilds the archives — a stale toolchain never survives a
  version change.
- `version` selects mruby only; every other toolchain has no version
  setting — its selected version is the one the installed beni release
  vendors.
- `beni:vendor:setup` unpacks toolchains from the tarball cache and
  downloads only the tarballs the cache lacks.
- `toolchains` names what the consumer requests; beni resolves transitive
  dependencies automatically (selecting `wasi-sdk` implies `mruby`).
- `beni:build` builds every target the build config defines, then verifies
  that each name in `targets` produced an archive and its compile-flags
  sidecar; targets beyond `targets` are not verified. The config owns the
  target definitions, and beni never reads the config.
- Toolchains unpack at their own names under the vendor tree (the mruby
  source at `mruby/`); each target's archive and its compile-flags sidecar
  stage at `mruby/build/<name>/lib/` under the vendor tree — the staged
  path.
- The crates auto-discover one archive: the `host` build's, serving host
  cargo targets. Build configs may declare additional or differently named
  targets, but every archive beyond `host` — every cross-compiled target's
  archive included — is reachable only via `MRUBY_LIB_DIR`.
- Customization goes through `beni:config`, which writes a self-contained
  equivalent of the configured `version`'s upstream default config to the
  path the `build_config` setting names. The generated file requires nothing
  from beni at build time, builds without edits, and belongs to the consumer
  — beni never rewrites it. Generation creates the target path's missing
  parent directories and refuses to overwrite an existing file.
- Every build writes each archive's compile-flags sidecar; the sidecar is
  the single alignment channel to the crates.

### beni-sys crate — FFI surface

- FFI bindings are generated against the discovered archive and aligned via
  the compile-flags sidecar, so the bindings always match how the archive was
  actually built. The crate follows the `-sys` crate convention.
- One archive serves one cargo build target. Archive discovery is
  environment-driven, highest precedence first; the highest-precedence
  variable set is the sole source, never falling back to a lower one:
  1. `MRUBY_LIB_DIR` — the `-sys` crate `*_LIB_DIR` convention — names the
     directory containing the active target's archive and compile-flags
     sidecar.
  2. `BENI_VENDOR_DIR` names the vendor tree the gem populated; the crate
     reads the `host` build's staged path and serves host cargo targets
     only — a cross-compiled cargo target never reads the vendor tree and
     requires `MRUBY_LIB_DIR`.
  3. With neither variable set, no archive is linked: a host build compiles
     in placeholder mode, a cross-compiled build fails.
- wasm32 is the one supported cross target and requires the wasi-sdk
  toolchain: `WASI_SDK_PATH` names its unpacked root, defaulting to
  `wasi-sdk/` under the tree `BENI_VENDOR_DIR` names; when neither
  variable is set, the toolchain is missing.
- Supports one FFI surface per mruby minor version; supported versions: 4.0.
- In placeholder mode `cargo check` passes and no FFI surface is exported.
- A `mruby_linked` cfg reflects whether a real archive is linked; downstream
  crates read it to gate their mruby-dependent code. It is capability-driven,
  never a cargo feature.

### beni crate — typed wrapper

- Owns every Rust-level abstraction over the C API: an RAII interpreter
  handle (`Mrb`, opened via `Mrb::open`), `Value` newtypes with typed
  conversions (`IntoValue` / `FromValue`), class and module definition, and
  closure-based exception protection.
- Class and module definition are methods on the live `Mrb` handle:
  `define_class(name, superclass)` and `define_module(name)` return typed
  `Class` and `Module` handles. Methods are registered on those handles
  through the `Class` and `Module` traits (mirroring `magnus::Module` and
  `magnus::Object`), accepting Rust closures whose arguments and return
  values cross the boundary through `IntoValue` / `FromValue`.
- Provides the `Gem` trait — the unit of Ruby surface a Rust crate ships:

  ```rust
  trait Gem {
      fn init(mrb: &Mrb) -> Result<(), Error>;
  }
  ```

  The embedder invokes each gem's `init` with the live interpreter handle
  during interpreter setup; the gem defines its classes, modules, and methods
  there. An `Err` from `init` aborts setup and surfaces to the embedder.
- The safe API cannot cause undefined behavior. Any C API the safe wrapper
  does not expose is reachable through the re-exported `beni::sys` escape
  hatch; using `beni::sys` directly is unsafe and outside the wrapper's
  guarantees.
- In placeholder mode the wrapper's full API surface still compiles;
  `Mrb::open` returns an error, so no interpreter ever exists to operate
  on.

## Error scenarios

| Scenario | Behavior |
|---|---|
| `toolchains` naming anything other than `mruby` or `wasi-sdk` | `beni:vendor:setup` aborts before any download |
| Toolchain download fails (network failure, HTTP 4xx/5xx, disk write error) | `beni:vendor:setup` aborts, no partial unpack, the vendor tree is left in its pre-setup state |
| Toolchain download fails checksum verification | `beni:vendor:setup` aborts, no partial unpack, the vendor tree is left in its pre-setup state |
| `build_config` naming a path that does not exist | `beni:build` aborts and names the missing config path, no archive built |
| `beni:build` with `targets` naming a target the build config does not define | verification fails, each missing archive reported |
| `beni:config` with `build_config` left at its `nil` default | task fails, nothing generated |
| `beni:config` targeting an existing file | generation refuses, existing config untouched |
| Discovered archive missing its compile-flags sidecar | `beni-sys` build fails and names the compile-flags sidecar, never silently falls back to placeholder mode |
| `MRUBY_LIB_DIR` or `BENI_VENDOR_DIR` set but the archive is absent | `beni-sys` build fails and names the expected path, never falls back to placeholder mode |
| Discovered archive at an mruby version outside the supported versions | `beni-sys` fails to compile, never falls back to placeholder mode |
| Cross-compiled build without `MRUBY_LIB_DIR` | `beni-sys` build fails, never falls back to placeholder mode |
| wasm32 build missing its archive or the wasi-sdk toolchain | `beni-sys` build fails, never falls back to placeholder mode |
| `WASI_SDK_PATH` set but the named root lacks the wasi-sdk toolchain | `beni-sys` build fails and names the root, never falls back to placeholder mode |
| `Mrb::open` without a linked mruby | returns an error, never aborts |
| Ruby exception raised inside protected execution | surfaced as a Rust `Err`, never unwinds across FFI |
| Rust panic raised inside any closure the safe wrapper invokes (`Gem::init` body, registered method, exception-protected closure) | caught at the FFI boundary; surfaced as a Rust `Err` when the caller is Rust, or as an mruby exception when the caller is mruby; never unwinds into mruby's C frames |
| Registered method receiving an argument that fails `FromValue` conversion | raised as an mruby exception to the Ruby caller, the closure body never runs |
| `Gem::init` returns `Err` | interpreter setup aborts, the error surfaces to the embedder |

## Terminology

| Term | Meaning |
|---|---|
| toolchain | a vendored build dependency (mruby source, wasi-sdk) |
| vendor tree | the directory tree the `vendor_dir` setting names |
| tarball cache | downloaded toolchain tarballs, kept inside the vendor tree |
| archive | the built `libmruby.a` for one target |
| discovered archive | the archive located by archive discovery for the active cargo target |
| archive discovery variable | `MRUBY_LIB_DIR` or `BENI_VENDOR_DIR`, the environment variables archive discovery consults |
| staged | present in the vendor tree and ready to consume — toolchains unpacked, archives built |
| compile-flags sidecar | `libmruby.flags.mak`, the per-archive record of defines/flags the crates align with |
| placeholder mode | host crate compilation with no archive linked — entered only when no archive discovery variable is set |
