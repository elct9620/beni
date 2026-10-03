# beni

Typed Rust wrapper over the mruby C API, the Rust half of
[beni](https://github.com/elct9620/beni). As magnus sits over rb-sys,
[beni-sys](https://crates.io/crates/beni-sys) carries the bindgen FFI
surface and this crate owns every abstraction above it.

## Surface

Each item is reached from the crate root.

| Item | What it is |
|---|---|
| `Mrb` / `Ccontext` | RAII owners of the interpreter and compile contexts |
| `Value` and its handles | `RClass`, `RArray`, `Integer`, … — one per type tag |
| `FromValue` | the downcast that tells a value's type |
| `IntoValue` / `TryConvert` | Rust into a value; the argument conversion |
| `TypedData` | a Rust payload read back as `&T` or `Obj<T>` |
| `InlineStruct` | a `Pod` struct stored inside the object |
| `method!` | registers a typed Rust function as a method |
| `beni::sys` | the raw FFI, with `protect` and `catch_unwind` |

`#[beni::wrap]` and `#[derive(beni::TypedData)]` implement `TypedData`
for the class `mark_carriers` prepares from the gem's `init`.
`#[derive(beni::InlineStruct)]` or `#[beni::wrap(..., inline)]` does the
same for `InlineStruct`, copied through `Inline<T>`. `method!` converts
the arguments and seals the panic boundary. `sys::protect` turns a raw
binding's raise, and `sys::catch_unwind` a C callback's panic, into an
`Err`.

## Usage

<!-- x-release-please-start-version -->
```toml
[dependencies]
beni = "0.19.0"
```
<!-- x-release-please-end -->

The `compiler` feature is on by default and carries what mruby keeps in
its compiler gem — `Ccontext` and `Mrb::load_string`. Turn default
features off to embed mruby without compiling Ruby at run time; loading
precompiled bytecode needs no compiler and stays.

<!-- x-release-please-start-version -->
```toml
[dependencies]
beni = { version = "0.19.0", default-features = false }
```
<!-- x-release-please-end -->

The `bytes` feature, off by default, converts `bytes::Bytes` to and from
an mruby String.

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

`beni-sys` finds a prebuilt archive through one of two variables, and
fails naming both when neither leads to one.

| Variable | Points at |
|---|---|
| `MRUBY_LIB_DIR` | the directory holding the archive |
| `BENI_VENDOR_DIR` | the vendor tree the beni Ruby gem stages |

The `libmruby.flags.mak` sidecar beside the archive aligns the bindings
with its ABI. Behavior contracts live in the repository's
[SPEC.md](https://github.com/elct9620/beni/blob/main/SPEC.md).

## License

Apache-2.0
