# beni-sys

bindgen-driven FFI bindings to the mruby C API — the rb-sys half of
the magnus / rb-sys split that [beni](https://crates.io/crates/beni)
applies at the mruby boundary. Most consumers want the typed `beni`
crate; this one stays a pure FFI surface of bindings, ABI constants, and
layout-safe C shims. Behavior contracts live in the
repository's [SPEC.md](https://github.com/elct9620/beni/blob/main/SPEC.md).

```
beni         typed wrapper     (magnus)
  │
beni-sys     FFI bindings      (rb-sys)
  │
archive      + libmruby.flags.mak
```

## Archive Discovery

The build script finds a prebuilt archive and aligns the bindings with
its compile flags. The `libmruby.flags.mak` sidecar beside the archive
is the sole ABI alignment channel.

| Variable | Points at |
|---|---|
| `MRUBY_LIB_DIR` | the archive and its sidecar; required for cross targets |
| `BENI_VENDOR_DIR` | the beni gem's `beni:build` vendor tree; host builds |
| `WASI_SDK_PATH` | wasi-sdk for `wasm32-wasip1`; default `/opt/wasi-sdk` |

A build that finds no archive fails naming these variables.

## Documentation Builds

A documentation build, marked by `DOCS_RS`, stages no archive and never
links. It reads the `src/bindings_docs.rs` the published crate carries.

| Build | Type widths shown |
|---|---|
| docs.rs page | mruby's own default config |
| your build | the archive it discovers |

## License

Apache-2.0; see the repository's [LICENSE](https://github.com/elct9620/beni/blob/main/LICENSE).
