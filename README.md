# Beni

beni gives Rust developers a magnus-like experience for mruby: a Ruby gem
manages the mruby build chain, and Rust crates expose a safe, typed API over
the resulting `libmruby.a`. Extracted from the
[kobako](https://github.com/elct9620/kobako) project; APIs follow 0.x semver
semantics and may still evolve between minor versions.

## Packages

All three packages release in lockstep under a single version.

| Package | Registry | Role |
|---|---|---|
| `beni` gem | rubygems.org | Rake tasks + DSL config that download mruby and build `libmruby.a` |
| `beni-sys` crate | crates.io | bindgen FFI surface over the mruby C API |
| `beni` crate | crates.io | safe typed wrapper over `beni-sys`, aligned with magnus idioms |

## Getting started

### Build `libmruby.a` with the gem

Add `beni` to your Gemfile and install the task library in your Rakefile:

```ruby
require "beni/tasks"

Beni::Tasks.new
```

```bash
rake beni:build
```

This downloads the pinned mruby release, builds it with mruby's untouched
upstream default config, and stages `vendor/mruby/build/host/lib/` with
`libmruby.a` and its `libmruby.flags.mak` compile-flags sidecar — everything
the crates need.

To tune the build, declare a config path and generate the seed:

```ruby
Beni::Tasks.new do
  build_config "build_config/mruby.rb"
end
```

`rake beni:config` writes a self-contained copy of the upstream default
config to that path. The file is yours to edit — add targets, gems, or
defines; beni never rewrites it.

### Embed mruby from Rust

Add the `beni` crate to your Cargo.toml, then point archive discovery at
the vendor tree the gem staged:

```bash
BENI_VENDOR_DIR=$PWD/vendor cargo build
```

A crate ships its Ruby surface as a `Gem` and installs it during
interpreter setup:

```rust
use beni::{method, Error, Gem, Module, Mrb, Value};

fn answer(_mrb: &Mrb, _self: Value) -> i32 {
    42
}

struct WidgetGem;

impl Gem for WidgetGem {
    fn init(mrb: &Mrb) -> Result<(), Error> {
        let widget = mrb.define_class(c"Widget", mrb.object_class())?;
        widget.define_method(mrb, c"answer", method!(answer, 0))?;
        Ok(())
    }
}

fn main() {
    let mrb = Mrb::open().expect("mruby interpreter");
    mrb.init_gem::<WidgetGem>().expect("Widget surface");
    // Widget#answer now returns 42 to any Ruby code the interpreter runs.
}
```

With no archive discovery variable set, a host build compiles in
placeholder mode: `cargo check` passes, no FFI surface is exported, and
`Mrb::open` returns an error — so `beni` is safe to take as a transitive
dependency. Any C API the typed wrapper does not cover stays reachable
through the unsafe `beni::sys` escape hatch.

### Cross-compile for wasm32-wasip1

Declare a `wasi` target referencing the `wasi-sdk` toolchain:

```ruby
Beni::Tasks.new do
  build_config "build_config/mruby.rb"

  target :host
  target :wasi do
    toolchain "wasi-sdk"
  end
end
```

and append the cross build to the generated config:

```ruby
MRuby::CrossBuild.new("wasi") do |conf|
  conf.toolchain :wasi
end
```

`conf.toolchain :wasi` resolves to the wasi toolchain file
`beni:vendor:setup` stages into the mruby tree whenever `wasi-sdk` is
selected — the cross-compile settings ship with beni and update with it.
After `rake beni:build`, name the staged archive and the wasi-sdk root
explicitly for the cargo side (a cross-compiled cargo target never reads
the vendor tree on its own):

```bash
MRUBY_LIB_DIR=$PWD/vendor/mruby/build/wasi/lib \
WASI_SDK_PATH=$PWD/vendor/wasi-sdk \
cargo build --target wasm32-wasip1
```

## Toolchain

beni targets plain mruby and is not bound to WebAssembly. `rust-toolchain.toml`
keeps `wasm32-wasip1` only as a build-verification target for downstream wasi
consumers (kobako). For that target the Rust channel and the wasi-sdk version
move in lockstep (the wasm32-wasip1 `crt1-command.o` references
`__wasi_init_tp` from Rust 1.96 onward; wasi-sdk 33's `libc.a` supplies that
symbol) — bump the pair together, in both this repo and kobako. Host builds
are unaffected by the pairing.

## Development

After checking out the repo, run `bin/setup` to install dependencies, then
`bundle exec rake` for the default gate (tests + RuboCop + Steep). The repo
dogfoods its own gem: the Rakefile wires `Beni::Tasks` with the validation
config `build_config/mruby.rb` (host + wasi targets), and a repo-local rake
chain verifies the crates compile against a real `libmruby.a` on both the
host target and wasm32-wasip1:

```bash
bundle exec rake rust:verify   # beni:build + check/test (host) + check (wasm32)
```

Behavior contracts live in `SPEC.md` — the source of truth the
implementation follows.

## Releasing

Releases are cut by release-please: merging the release PR tags the
version and publishes the gem and both crates in lockstep through OIDC
trusted publishing. One-time cleanup: after 0.1.0 ships, remove the
`release-as` line (and the then-stale `last-release-sha`) from
`release-please-config.json` — left in place it pins every subsequent
release to 0.1.0.

## Contributing

Bug reports and pull requests are welcome on GitHub at
https://github.com/elct9620/beni.

## License

The gem and crates are available as open source under the terms of the
[Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0).
