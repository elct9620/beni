# beni

Typed Rust wrapper over the mruby C API, the Rust half of
[beni](https://github.com/elct9620/beni). As magnus sits over rb-sys,
[beni-sys](https://crates.io/crates/beni-sys) carries the bindgen FFI
surface and this crate owns every abstraction above it.

```text
magnus ──over──▶ rb-sys      (CRuby)
beni   ──over──▶ beni-sys    (mruby)
```

## Surface

Each item is reached from the crate root.

| Item | What it is |
|---|---|
| `Mrb` / `Ccontext` | RAII owners of the interpreter and compile contexts |
| `Value` and its handles | `RClass`, `RArray`, `Integer`, … — one per type tag |
| `FromValue` | the downcast that tells a value's type |
| `IntoValue` / `TryConvert` | Rust into a value; the argument conversion |
| `TryConvertOwned` | the owned targets a `Vec` or map converts to |
| `value::Lazy` | a value a `static` names, held per interpreter |
| `TypedData` | a Rust payload read back as `&T` or `Obj<T>` |
| `InlineStruct` | a `Pod` struct stored inside the object |
| `method!` | registers a typed Rust function as a method |
| `Mrb::proc_from_fn` | a `Proc` whose body is a Rust closure |
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
beni = "0.20.0"
```
<!-- x-release-please-end -->

Each default feature carries what mruby keeps in one of its gems. Turn
default features off when the archive leaves those gems out; loading
precompiled bytecode needs no compiler and stays.

| Feature | Default | Carries |
|---|---|---|
| `compiler` | on | `Ccontext`, `Mrb::load_string` |
| `fiber` | on | `Mrb::fiber_new`, `Mrb::fiber_current`, `Mrb::fiber_yield` |
| `bytes` | off | `bytes::Bytes` to and from String |

<!-- x-release-please-start-version -->
```toml
[dependencies]
beni = { version = "0.20.0", default-features = false }
```
<!-- x-release-please-end -->

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

Licensed under [Apache-2.0](LICENSE).
