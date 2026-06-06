# beni-sys

bindgen-driven FFI bindings to the mruby C API — the rb-sys half of
the magnus / rb-sys split that [beni](https://crates.io/crates/beni)
applies at the mruby boundary. Most consumers want the typed `beni`
crate; this one stays a pure FFI surface (bindings, ABI constants,
layout-safe C shims).

The build script discovers a prebuilt `libmruby.a` and aligns the
generated bindings with the archive's compile flags through the
`libmruby.flags.mak` sidecar — the sole ABI alignment channel:

- `MRUBY_LIB_DIR` — directory holding `libmruby.a` and its sidecar;
  required for cross-compiled targets
- `BENI_VENDOR_DIR` — vendor tree staged by the beni Ruby gem's
  `beni:build` task; serves host builds
- `WASI_SDK_PATH` — wasi-sdk root for `wasm32-wasip1` cross builds
  (defaults to `/opt/wasi-sdk`)

Without an archive the host build emits a placeholder surface so
downstream `cargo check` passes. `links = "mruby"` publishes the
outcome as `DEP_MRUBY_LINKED` (`1`/`0`) for downstream build scripts.

Behavior contracts live in the repository's
[SPEC.md](https://github.com/elct9620/beni/blob/main/SPEC.md).

## License

Apache-2.0
