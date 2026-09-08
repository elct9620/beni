# beni

Typed Rust wrapper over the mruby C API — the Rust half of
[beni](https://github.com/elct9620/beni), an mruby toolchain whose
Ruby gem builds `libmruby.a` and whose crates bind it. The split
mirrors magnus + rb-sys at the CRuby boundary:
[beni-sys](https://crates.io/crates/beni-sys) carries the bindgen FFI
surface, this crate owns every abstraction above it.

- `Mrb` / `Ccontext` — RAII owners of the interpreter state and
  compile contexts
- `Value` / `RClass` / `RModule` / `Array` / `Hash` — typed handles
  over `mrb_value`
- `IntoValue` / `FromValue` — the Rust ⇄ mruby conversion seam
- `method!` — registers a typed Rust function as an mruby method,
  with argument conversion and a sealed panic boundary
- `protect` — closure-based `mrb_protect_error`, surfacing mruby
  exceptions as Rust `Err`
- `beni::sys` — raw-FFI escape hatch re-exporting all of `beni-sys`

## Usage

```toml
[dependencies]
beni = "0.1"
```

The `compiler` feature is on by default and carries what mruby keeps in
its compiler gem — `Ccontext` and `Mrb::load_string`. Turn default
features off to embed mruby without compiling Ruby at run time; loading
precompiled bytecode needs no compiler and stays.

```toml
[dependencies]
beni = { version = "0.1", default-features = false }
```

```rust
use beni::{Module, Mrb, Value};

fn add(_mrb: &Mrb, _self: Value, a: i32, b: i32) -> i32 {
    a + b
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mrb = Mrb::open()?;
    let calc = mrb.define_class(c"Calc", mrb.object_class())?;
    calc.define_method(&mrb, c"add", beni::method!(add, 2))?;
    Ok(())
}
```

## Linking mruby

`beni-sys` discovers a prebuilt archive through environment variables
(`MRUBY_LIB_DIR`, or the vendor tree the beni Ruby gem stages under
`BENI_VENDOR_DIR`) and aligns its bindings with the archive's ABI via
the `libmruby.flags.mak` sidecar. A build that finds no archive fails
naming the variables it consulted.

Behavior contracts live in the repository's
[SPEC.md](https://github.com/elct9620/beni/blob/main/SPEC.md).

## License

Apache-2.0
