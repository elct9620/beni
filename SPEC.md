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
- Once a target declaration references `wasi-sdk`, a build config
  cross-compiles for wasm32-wasip1 with `conf.toolchain :wasi` — the
  cross-compile settings ship with beni and update with it, instead of
  living hand-maintained inside the consumer's config.
- Under one installed beni release, the same `version`, `build_config`,
  `target`, and `toolchain` declarations always build the same way: the
  same toolchain versions, compile flags, and staged layout.
- In a host build with no archive discovery variable set, a crate that
  depends on `beni` compiles in placeholder mode, so `beni` is safe to
  take as a transitive dependency in such builds.

## Success criteria

- A fresh checkout running `rake beni:build` produces `libmruby.a` and its
  compile-flags sidecar at the staged path for every target the build
  config defines.
- A Rust binary built with `BENI_VENDOR_DIR` pointing at that vendor tree
  links the archive and runs an mruby interpreter through `Mrb::open`.
- `cargo check` on the `beni` crate succeeds with no archive discovery
  variable set, and `Mrb::open` returns an error.
- A `wasm32-wasip1` cross-build succeeds when a target declaration
  references `wasi-sdk`, the build config defines a target cross-compiled
  for wasm32, `MRUBY_LIB_DIR` names that target's staged path, and
  `WASI_SDK_PATH` names the unpacked wasi-sdk root.

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
| `beni-sys` crate | crates.io | `-sys` style FFI surface over the mruby C API, generated against the discovered archive per supported mruby version |
| `beni` crate | crates.io | safe typed wrapper over `beni-sys`, aligned with magnus idioms |

Responsibility boundary: the gem stages toolchains and archives; `beni-sys`
binds them; the `beni` crate is the only package consumers write Rust against.

## Features

### beni gem — toolchain management

Consumers install the task library in their Rakefile:

```ruby
require "beni/tasks"

Beni::Tasks.new do
  version "4.0.0"
  build_config "build_config/mruby.rb"

  target :host
  target :wasi do
    toolchain "wasi-sdk"
  end

  toolchain "wasi-sdk" do
    version "29"
    sha256 "…"
  end
end
```

| Setting | Declared as | Default |
|---|---|---|
| `vendor_dir` | `vendor_dir <path>` — where toolchains unpack and mruby builds; relative paths resolve against the Rakefile's working directory | `vendor/` under the Rakefile's working directory. `BENI_VENDOR_DIR` env var overrides the default; an explicit declaration overrides the env var. |
| `version` | `version <string>` — the mruby release version to download | `"4.0.0"` |
| `build_config` | `build_config <path>` — mruby build-config file path; relative paths resolve against the Rakefile's working directory | undeclared — mruby's untouched upstream default config |
| targets | `target <name>`, optionally with a block of toolchain references — each declaration names one build target to verify, matching the `MRuby::Build.new(<name>)` names in the config; a build defined without a name is named `host` by mruby | `host` when no `target` declaration appears; any `target` declaration replaces the default — the declared set is the whole set |
| toolchains | a block-less `toolchain <name>` inside a target block — a toolchain reference; `toolchain <name> do … end` at the top level carrying `version` and `sha256` — a toolchain definition | selection is reference-driven; every toolchain other than `mruby` defaults to its built-in pair |

| Task | Outcome |
|---|---|
| `beni:build` | toolchains staged, `libmruby.a` built per target |
| `beni:clean` | mruby build trees removed, vendored source kept |
| `beni:config` | self-contained, editable build config generated at the `build_config` path |
| `beni:vendor:setup` | selected toolchains downloaded and unpacked; the wasi toolchain file staged when `wasi-sdk` is selected |
| `beni:vendor:clean` | unpacked toolchains removed, tarball cache kept |
| `beni:vendor:clobber` | vendor tree removed entirely, tarball cache included |

Behaviors:

| Behavior | Contract |
|---|---|
| Version convergence | The vendor tree converges on each toolchain's selected version: a staged toolchain at any other version is replaced by `beni:vendor:setup`, and `beni:build` rebuilds the archives — a stale toolchain never survives a version change. |
| Toolchain unpack | `beni:vendor:setup` unpacks toolchains from the tarball cache and downloads only the selected versions' tarballs the cache lacks; every tarball it unpacks — cached or freshly downloaded — must match its toolchain's selected checksum. |
| Toolchain selection | Reference-driven: the selected set is every target declaration's toolchain references plus the transitive dependencies beni resolves automatically (referencing `wasi-sdk` implies `mruby`); `mruby` is always selected. A toolchain definition selects nothing by itself — a definition for a toolchain nothing references is inert. |
| Build & verify | `beni:build` builds every target the build config defines, then verifies that each declared target produced an archive and its compile-flags sidecar; a target no `target` declaration names is not verified. The config owns the target definitions, and beni never reads it. |
| Staged path | Toolchains unpack at their own names under the vendor tree (the mruby source at `mruby/`); each target's archive and its compile-flags sidecar stage at `mruby/build/<name>/lib/` — the staged path. |
| Archive auto-discovery | The crates auto-discover one archive: the `host` build's, serving host cargo targets. An archive beyond `host` is never auto-discovered and is reachable only via `MRUBY_LIB_DIR`. |
| Compile-flags sidecar | Every build writes each archive's sidecar; it is the single ABI-alignment channel to the crates. |

Selection, checksums, and cross-compile activation:

- `version` selects mruby; a toolchain definition never names `mruby`.
  Every other toolchain's selected version and checksum default to its
  built-in pair; a toolchain definition replaces both. A toolchain
  released as one tarball per build platform downloads the build
  platform's tarball: its built-in pair vendors one checksum per
  tarball and the selected checksum is the downloaded tarball's; a
  toolchain definition's single `sha256` becomes the selected checksum
  on every build platform — it verifies only the tarball it names.
  mruby's selected checksum is the one the installed release vendors
  for the default `version`; for any other `version` it is the pin
  that `version`'s first download establishes. The pin persists
  alongside the tarball cache and shares its lifecycle; once
  `beni:vendor:clobber` removes both, the next download establishes a
  new pin.
- Every `beni:vendor:setup` run with `wasi-sdk` selected writes the wasi
  toolchain file into the staged mruby source, so a re-extracted tree
  never lacks it. The file carries beni's wasm32-wasip1 cross-compile
  settings; a build config activates them with `conf.toolchain :wasi`
  inside its cross-build definition and needs no toolchain setup of its
  own. The settings resolve the wasi-sdk root from `WASI_SDK_PATH` when
  set, the vendor tree's unpacked `wasi-sdk` otherwise.
- `beni:config` seeds customization: it writes a self-contained equivalent
  of the configured `version`'s upstream default config to the path the
  `build_config` declaration names. The generated file requires nothing from
  beni at build time, builds without edits, and belongs to the consumer,
  who edits it to define further targets — cross-compiled ones included;
  beni never rewrites the file. Generation creates the target path's
  missing parent directories and refuses to overwrite an existing file.

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
- wasm32 is the one supported cross target; a build for any other
  cross-compiled cargo target fails and names the unsupported target.
  wasm32 requires the wasi-sdk toolchain: `WASI_SDK_PATH` names its
  unpacked root, defaulting to `/opt/wasi-sdk` when the variable is
  unset.
- Supports one FFI surface per mruby minor version; supported versions: 4.0.
- In placeholder mode `cargo check` passes and no FFI surface is exported.
- A `mruby_linked` cfg reflects whether a real archive is linked. The cfg
  is derived, never a cargo feature: `beni-sys` publishes the linked
  signal to its direct dependents' build scripts in every build, the
  `beni` crate re-derives its own cfg from the signal's value
  automatically, and any crate gating mruby-dependent code does the same
  as a direct dependent of `beni-sys`.

### beni crate — typed wrapper

- Owns every Rust-level abstraction over the C API: an RAII interpreter
  handle (`Mrb`, opened via `Mrb::open`), `Value` newtypes with typed
  conversions (`IntoValue`, a total conversion that cannot fail;
  `FromValue`, a checked conversion that can reject), class and module
  definition, and closure-based exception protection.
- `FromValue` downcasts to the typed handles (`Array`, `Hash`, `RClass`,
  `Proc`, `Symbol`) discriminate by mruby's type tag alone: a value carrying
  the target's tag converts (for the containers, subclass instances included),
  any other tag rejects.
- Class and module definition are methods on the live `Mrb` handle:
  `define_class(name, superclass)` and `define_module(name)` return typed
  `RClass` and `RModule` handles. Methods are registered on those handles
  through the `Module` and `Object` traits (mirroring `magnus::Module` and
  `magnus::Object`), accepting Rust closures whose arguments and return
  values cross the boundary through `IntoValue` / `FromValue`; the `Module`
  trait also binds constants and aliases existing methods on the handle. A
  definition, registration, or alias mruby rejects surfaces as a Rust `Err`.
- A Rust-owned value backs an mruby object through the data-carrier
  mechanism (`CDATA`): a class is marked so its instances carry Rust data,
  a Rust value is wrapped as an instance of that class, and it is extracted
  back type-checked against the data type it was registered under — a value
  carrying a different data type, or none, does not extract. The mruby
  garbage collector owns the wrapped value's lifetime, releasing it when
  its carrier is collected. Mirrors `magnus`'s typed-data wrapping, and
  meets the graduation bar — correct use needs no reasoning about VM
  internals — so it lives on the typed surface rather than behind
  `beni::sys`.
- Provides the `Gem` trait — the unit of Ruby surface a Rust crate ships:

  ```rust
  trait Gem {
      fn init(mrb: &Mrb) -> Result<(), Error>;
  }
  ```

  The embedder invokes each gem's `init` with the live interpreter handle
  during interpreter setup; the gem defines its classes, modules, and methods
  there. An `Err` from `init` aborts setup and surfaces to the embedder.
- `Mrb::arena_scope` bounds GC arena growth across a region of Rust code:
  values created inside the scope hold arena protection until the scope
  ends, and the scope's end releases it. `keep` ends the scope and
  re-protects the one value it names; dropping the scope ends it with no
  survivor.
- A typed `Proc` handle wraps an mruby block. `Proc::call` invokes it with
  an argument slice under the same exception protection as closure-based
  `protect`: the block's normal return is the `Ok` value, and any non-local
  exit — a raised exception, or a `break` / `return` object the block throws
  — surfaces as a Rust `Err` instead of unwinding across FFI. `Value::as_break`
  views an escaped value as a typed `Break` when it carries mruby's break tag
  and yields no view for any other tag; `Break` exposes the value the break
  carries. Whether a break is a real `break`, a `return` aimed past a frame, or
  a plain raise is the consumer's classification. The call-info frame indices
  that distinguish those cases are mruby VM internals with no stable public
  accessor; the typed surface does not expose them, so a consumer that must
  classify reaches them through the `beni::sys` escape hatch.
- The safe API cannot cause undefined behavior while the GC validity rule
  holds: a value created inside an arena scope is not used after that
  scope ends, and a survivor carried out through `keep` counts as created
  where its scope was opened. The type system does not enforce the rule;
  the consumer upholds it.
- The typed surface graduates a capability out of the `beni::sys` escape
  hatch only when a caller can use it correctly without reasoning about
  mruby's VM internals — a stronger bar than freedom from undefined
  behavior. An operation whose correct use depends on internal VM structure
  (raw call-info frame indices, VM-object internals) stays behind
  `beni::sys`: a safe-looking typed wrapper would misrepresent its
  sharpness, so it stays where `unsafe` marks the caller's responsibility
  for the invariants the type system cannot check. Any C API the typed
  surface does not expose remains reachable there, unsafe and outside the
  wrapper's guarantees; closing a consumer's `beni::sys` use to zero is not
  a goal.
- In placeholder mode the wrapper's full API surface still compiles;
  `Mrb::open` returns an error, so no interpreter ever exists to operate
  on.

## Error scenarios

| Scenario | Behavior |
|---|---|
| A toolchain reference or definition naming anything other than `mruby` or `wasi-sdk` | `Beni::Tasks.new` fails, no task defined, nothing downloaded |
| A toolchain definition naming `mruby` | `Beni::Tasks.new` fails, no task defined, nothing downloaded |
| A toolchain definition missing its `version` or `sha256` | `Beni::Tasks.new` fails, no task defined, nothing downloaded |
| A block-carrying `toolchain` declaration inside a target declaration's block | `Beni::Tasks.new` fails, no task defined, nothing downloaded |
| A block-less `toolchain` declaration at the top level | `Beni::Tasks.new` fails, no task defined, nothing downloaded |
| More than one toolchain definition naming the same toolchain | `Beni::Tasks.new` fails, no task defined, nothing downloaded |
| More than one `target` declaration naming the same target | `Beni::Tasks.new` fails, no task defined, nothing downloaded |
| More than one declaration of the same setting (`version`, `build_config`, or `vendor_dir`) | `Beni::Tasks.new` fails, no task defined, nothing downloaded |
| Toolchain download fails (network failure, HTTP 4xx/5xx, disk write error) | `beni:vendor:setup` aborts, no partial unpack, the vendor tree is left in its pre-setup state |
| A downloaded or cached tarball fails checksum verification | `beni:vendor:setup` aborts, no partial unpack, the vendor tree is left in its pre-setup state |
| `build_config` naming a path that does not exist | `beni:build` aborts and names the missing config path, no archive built |
| `beni:build` with a `target` declaration naming a target the build config does not define | verification fails, each missing archive reported |
| A build config selecting the `wasi` toolchain with no wasi toolchain file staged | `beni:build` aborts, mruby naming the unknown toolchain |
| `beni:config` with no `build_config` declaration | task fails, nothing generated |
| `beni:config` with the configured `version`'s mruby source not staged | task fails and names the missing source, nothing generated |
| `beni:config` targeting an existing file | generation refuses, existing config untouched |
| Discovered archive missing its compile-flags sidecar | `beni-sys` build fails and names the compile-flags sidecar, never silently falls back to placeholder mode |
| `MRUBY_LIB_DIR` or `BENI_VENDOR_DIR` set but the archive is absent | `beni-sys` build fails and names the expected path, never falls back to placeholder mode |
| Discovered archive at an mruby version outside the supported versions | `beni-sys` fails to compile, never falls back to placeholder mode |
| Cross-compiled build for a cargo target other than wasm32 | `beni-sys` build fails and names the unsupported target, never falls back to placeholder mode |
| Cross-compiled build without `MRUBY_LIB_DIR` | `beni-sys` build fails, never falls back to placeholder mode |
| wasm32 build missing its archive or the wasi-sdk toolchain | `beni-sys` build fails, never falls back to placeholder mode |
| The wasi-sdk root in effect (`WASI_SDK_PATH` when set, `/opt/wasi-sdk` otherwise) lacks the wasi-sdk toolchain | `beni-sys` build fails and names the root, never falls back to placeholder mode |
| `Mrb::open` failing to produce an interpreter | returns an error, never aborts |
| Ruby exception raised inside protected execution | surfaced as a Rust `Err`, never unwinds across FFI |
| A block invoked through `Proc::call` exiting via a non-local `break` or `return` | the escaping mruby break object surfaces as a Rust `Err`, inspectable as a typed break view; beni does not classify the exit into an outcome |
| mruby raising during class or module definition, method registration, or method aliasing | surfaced as a Rust `Err`, never unwinds across FFI |
| Rust panic raised inside any closure the safe wrapper invokes (`Gem::init` body, registered method, exception-protected closure) | caught at the FFI boundary; surfaced as a Rust `Err` to the Rust caller (`Gem::init` body, exception-protected closure) or as an mruby exception to the Ruby caller (registered method); never unwinds into mruby's C frames |
| Registered method receiving an argument that fails `FromValue` conversion | raised as an mruby exception to the Ruby caller, the closure body never runs |
| `Gem::init` returns `Err` | interpreter setup aborts, the error surfaces to the embedder |

## Terminology

| Term | Meaning |
|---|---|
| toolchain | a vendored build dependency (mruby source, wasi-sdk) |
| target declaration | a `target <name>` entry in the Rakefile block — names one build target to verify; its own block holds the target's toolchain references |
| toolchain reference | a block-less `toolchain <name>` inside a target declaration's block — requests the named toolchain for vendoring |
| toolchain definition | a top-level `toolchain <name>` block carrying `version` and `sha256` — replaces the named toolchain's built-in pair |
| built-in pair | the version and checksum pair the installed beni release vendors for a toolchain; a toolchain released as one tarball per build platform vendors one checksum per tarball, the pair carrying the build platform's |
| build platform | the platform the Rake tasks run on; it selects which of a toolchain's per-platform tarballs is downloaded |
| vendor tree | the directory tree the `vendor_dir` setting names |
| tarball cache | downloaded toolchain tarballs, kept inside the vendor tree |
| archive | the built `libmruby.a` for one target |
| discovered archive | the archive located by archive discovery for the active cargo target |
| archive discovery variable | `MRUBY_LIB_DIR` or `BENI_VENDOR_DIR`, the environment variables archive discovery consults |
| staged | present in the vendor tree and ready to consume — toolchains unpacked, archives built |
| staged path | `mruby/build/<name>/lib/` under the vendor tree, holding one target's archive and compile-flags sidecar |
| wasi toolchain file | `tasks/toolchains/wasi.rake` under the staged mruby source — beni's wasm32-wasip1 cross-compile settings, staged whenever `wasi-sdk` is selected and activated by a build config via `conf.toolchain :wasi` |
| compile-flags sidecar | `libmruby.flags.mak`, the per-archive record of defines/flags the crates align with |
| linked signal | `DEP_MRUBY_LINKED`, the build-script metadata `beni-sys` publishes through its `links = "mruby"` key to direct dependents in every build — `1` with a real archive linked, `0` in placeholder mode |
| placeholder mode | host crate compilation with no archive linked — entered only when no archive discovery variable is set |
