# Beni Specification

## Purpose

Beni gives Rust developers a magnus-like experience for mruby: a Ruby gem
manages the mruby build chain, and Rust crates expose a safe, typed API over
the resulting `libmruby.a`.

## Users

- Rust developers who embed mruby and want typed, memory-safe APIs instead of
  raw FFI.
- Projects (e.g. kobako) that need a reproducible `libmruby.a` build wired
  into their own Rakefile.

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
| `beni-sys` crate | crates.io | bindgen-generated FFI surface over the mruby C API, per supported mruby version |
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
| `vendor_dir` | directory path — where toolchains unpack and mruby builds | `vendor/` under the Rakefile's working directory; `BENI_VENDOR_DIR` env var overrides |
| `build_config` | mruby build-config file path, or `nil` for mruby's upstream default | `nil` |
| `targets` | array of build-target names, matching the `MRuby::Build.new(<name>)` names in the config | `["host"]` |
| `toolchains` | array of toolchain names to vendor, from `mruby` and `wasi-sdk` | `["mruby"]` |

| Task | Outcome |
|---|---|
| `beni:build` | toolchains staged, `libmruby.a` built per target |
| `beni:clean` | mruby build trees removed, vendored source kept |
| `beni:config[path]` | self-contained, editable build config generated |
| `beni:vendor:setup` | configured toolchains downloaded and unpacked |
| `beni:vendor:clean` | unpacked toolchains removed, tarball cache kept |
| `beni:vendor:clobber` | vendor tree removed entirely |

Behaviors:

- A clean build with no `build_config` uses mruby's untouched upstream
  default config.
- Customization goes through `beni:config`: the generated file requires
  nothing from beni at build time, builds without edits, and belongs to the
  consumer — beni never rewrites it. Generation refuses to overwrite an
  existing file.
- Every build writes the compile-flags sidecar (`libmruby.flags.mak`) next to
  each archive; this sidecar is the single alignment channel to the crates.

### beni-sys crate — FFI surface

- bindgen runs against the staged archive and reads the compile-flags sidecar,
  so the generated bindings always match how the archive was actually built.
- Supports one FFI surface per mruby minor version; supported versions: 4.0.
- Without a staged archive, the crate compiles in placeholder mode: `cargo
  check` passes, no FFI surface is exported.
- A `mruby_linked` cfg reflects whether a real archive is linked; downstream
  crates read it to gate their mruby-dependent code. It is capability-driven,
  never a cargo feature.

### beni crate — typed wrapper

- Owns every Rust-level abstraction over the C API: an RAII interpreter
  handle (`Mrb`), `Value` newtypes with typed conversions
  (`IntoValue` / `FromValue`), class/module definition, and closure-based
  exception protection.
- Provides the `Gem` trait — the unit of Ruby surface a Rust crate ships:

  ```rust
  trait Gem {
      fn init(mrb: &Mrb) -> Result<(), Error>;
  }
  ```

  The embedder invokes each gem's `init` with the live interpreter handle
  during interpreter setup; the gem defines its classes, modules, and methods
  there. An `Err` from `init` aborts setup and surfaces to the embedder.
- The safe API cannot cause undefined behavior; anything not yet wrapped is
  reachable through the re-exported `beni::sys` escape hatch.
- In placeholder mode the wrapper compiles with the same modules gated by
  `mruby_linked`.

## Error scenarios

| Scenario | Behavior |
|---|---|
| Toolchain download fails checksum verification | build aborts, no partial unpack |
| `beni:build` with a config naming targets not in `targets` | verification fails, missing archives reported |
| `beni:config` targeting an existing file | generation refuses, existing config untouched |
| Staged archive missing its compile-flags sidecar | `beni-sys` build fails and names the sidecar, never silently falls back to placeholder mode |
| `Mrb::open` without a linked mruby | returns an error value, never aborts |
| Ruby exception raised inside protected execution | surfaced as a Rust `Err`, never unwinds across FFI |
| `Gem::init` returns `Err` | interpreter setup aborts, the error surfaces to the embedder |

## Terminology

| Term | Meaning |
|---|---|
| toolchain | a vendored build dependency (mruby source, wasi-sdk) |
| archive | the built `libmruby.a` for one target |
| compile-flags sidecar | the per-archive record of defines/flags the crates align with |
| placeholder mode | crate compilation without a staged archive |
