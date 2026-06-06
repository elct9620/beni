# Beni

mruby toolchain monorepo — a Ruby gem that manages the mruby build chain and
Rust crates that bind the mruby C API, extracted from the
[kobako](https://github.com/elct9620/kobako) project.

> **Status**: early stage. The gem ships the vendor + build chain
> (`Beni::Tasks`, `rake beni:config`) and the crates link and wrap the
> resulting `libmruby.a`. Published versions remain `0.0.0` until the first
> release cut; APIs may still change without notice.

## Packages

| Package | Registry | Role |
|---|---|---|
| `beni` gem | rubygems.org | mruby dependency manager — vendors mruby source, builds `libmruby.a`, future mrbgem management |
| `beni-sys` crate | crates.io | bindgen FFI surface over the mruby C API |
| `beni` crate | crates.io | typed Rust wrapper over `beni-sys` (magnus analog) |

## Toolchain

beni targets plain mruby and is not bound to WebAssembly. `rust-toolchain.toml`
keeps `wasm32-wasip1` only as a build-verification target for downstream wasi
consumers (kobako). For that target the Rust channel and the wasi-sdk version
move in lockstep (the wasm32-wasip1 `crt1-command.o` references
`__wasi_init_tp` from Rust 1.96 onward; wasi-sdk 33's `libc.a` supplies that
symbol) — bump the pair together, in both this repo and kobako. Host builds
are unaffected by the pairing.

## Development

After checking out the repo, run `bin/setup` to install dependencies. Then,
run `rake test` to run the tests. You can also run `bin/console` for an
interactive prompt that will allow you to experiment.

The Rust crates live under `crates/` in a Cargo workspace at the repo root.
The gem's own task library (`Beni::Tasks`, dogfooded by the Rakefile) stages
the toolchain, and a repo-local rake chain verifies the crates compile
against a real `libmruby.a` on both the host target and wasm32-wasip1:

```bash
bundle exec rake rust:verify   # beni:build + check/test (host) + check (wasm32)
```

`beni:vendor:setup` downloads the pinned mruby + wasi-sdk tarballs into
`vendor/`; `beni:build` produces `vendor/mruby/build/{host,wasi}/lib/libmruby.a`
from the repo's validation config `build_config/mruby.rb` (both targets pin the
same ABI-bearing defines — `MRB_INT32`, `MRB_WORDBOX_NO_INLINE_FLOAT`). That
config is the repo's own — `Beni::Tasks` defaults to no `MRUBY_CONFIG`, so a
consumer's clean build uses mruby's untouched upstream
`build_config/default.rb` (a single native `host` target). Consumers who need
to tune the build run `rake beni:config` to generate that upstream default
config — a self-contained, editable copy taken from the staged mruby source,
written to the path the `build_config` declaration in their `Beni::Tasks.new`
block names. The generated file is theirs to edit; the repo's own
`build_config/mruby.rb` is that seed hand-tuned into a host + wasi validation
harness.

To cross-compile for wasm32-wasip1, declare a `wasi` target referencing the
`wasi-sdk` toolchain:

```ruby
Beni::Tasks.new do
  build_config "build_config/mruby.rb"

  target :host
  target :wasi do
    toolchain "wasi-sdk"
  end
end
```

and append the cross build to the generated config — the same edit the
`generated_config` scenario applies:

```ruby
MRuby::CrossBuild.new("wasi") do |conf|
  conf.toolchain :wasi
end
```

`conf.toolchain :wasi` resolves to the wasi toolchain file
`beni:vendor:setup` stages into the mruby tree whenever `wasi-sdk` is
selected — the cross-compile settings (wasi-sdk tool paths, sysroot,
setjmp/longjmp flags) ship with beni and update with it. `WASI_SDK_PATH`
overrides the wasi-sdk root the staged settings point at.

The crates carry no hard-coded ABI defines: `beni-sys`'s build script parses
the `libmruby.flags.mak` sidecar mruby writes next to each archive (requested
by `Beni::Builder` on every build), so bindgen and the trampoline compile
always match what the archive was actually built with — whatever the config.
Without the staged toolchain, plain `cargo check --workspace` still passes in
a placeholder mode that exports no FFI surface.

## Contributing

Bug reports and pull requests are welcome on GitHub at
https://github.com/elct9620/beni.

## License

The gem and crates are available as open source under the terms of the
[Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0).
