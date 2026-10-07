# Beni Specification

## Purpose

Beni gives Rust developers a magnus-like experience for mruby. A Ruby gem manages the mruby build chain; Rust crates expose a safe, typed API over the archive it builds.

```
beni gem ──builds──> archive ──exposed by──> Rust crates (safe, typed API)
```

## Users

Beni serves two kinds of user.

| User | Need |
|---|---|
| Rust developers who embed mruby | typed, memory-safe APIs instead of raw FFI |
| Rakefile-based projects | a reproducible mruby archive build wired into their own build pipeline |

## Impacts

Each row names a mechanism and its effect.

| Mechanism | Effect |
|---|---|
| `beni` crate dependency | a Rust project calls mruby without writing or maintaining FFI declarations by hand |
| `rake beni:build` | a Rust project produces the archive without vendoring mruby source or scripting tarball downloads |
| `conf.toolchain :wasi` | once a target declaration references `wasi-sdk`, a build config cross-compiles for wasm32-wasip1 |
| wasm32-wasip1 cross-compile settings | ship with beni and update with it, instead of living hand-maintained inside the consumer's config |
| one installed beni release | the same `version`, `build_config`, `target`, and `toolchain` declarations always build the same way: same toolchain versions, compile flags, and staged layout |
| documentation host | renders the published crates without an archive, so the typed surface can be read before a build chain exists to try it against |
| disabling default features | a project embedding mruby without a gem a capability feature carries — compiling Ruby at run time, running fibers — drops the surface that needs that gem, rather than linking code it never calls |

## Success criteria

Beni succeeds when every condition yields its outcome.

| Condition | Outcome |
|---|---|
| fresh checkout runs `rake beni:build` | the archive and its compile-flags sidecar sit at the staged path for every target the build config defines |
| a consumer's own cargo project depends on the `beni` crate; `BENI_VENDOR_DIR` points at that vendor tree | it links the archive and runs a Ruby surface it defined through `Mrb::open` |
| `beni` crate behavior | verified from outside the crate, through its public paths alone, so every export those paths cross — the `sys` escape hatch among them — fails the suite the moment it stops being public |
| default features disabled; archive built without mruby's compiler and fiber gems | the `beni` crate builds and links; none of the capability features' surface is reachable |
| outside a documentation build; no archive discovery variable set | the build fails on every cargo target, naming the variables archive discovery consults |
| documentation build; no archive present | renders the typed surface from documentation bindings the published package carries and the repository does not |
| a target declaration references `wasi-sdk`; the build config defines a target cross-compiled for wasm32; `MRUBY_LIB_DIR` names that target's staged path; `WASI_SDK_PATH` names the unpacked wasi-sdk root | a `wasm32-wasip1` cross-build succeeds |
| the build config defines a target cross-compiled for the other macOS architecture; `MRUBY_LIB_DIR` names that target's staged path | a cross-build for that architecture succeeds |

## Non-goals

Beni excludes the following scope.

| Non-goal | Boundary |
|---|---|
| A WebAssembly project | wasm32-wasip1 is a downstream verification target only |
| Embedding mruby into Ruby programs | the gem only manages the toolchain for Rust consumers |
| CRuby extension support | magnus and rb-sys own that boundary |

## Packages

One repository; the gem and the three published crates release in lockstep under a single version number.

| Package | Registry | Responsibility |
|---|---|---|
| `beni` gem | rubygems.org | Rake tasks + DSL config that download mruby and build the archive for the crates to consume |
| `beni-sys` crate | crates.io | `-sys` style FFI surface over the mruby C API, generated against the discovered archive per supported mruby version |
| `beni` crate | crates.io | safe typed wrapper over `beni-sys`, aligned with magnus idioms |
| `beni-macros` crate | crates.io | the `beni` crate's attribute and derive macros, reached through the `beni` crate's re-exports as `magnus`'s are through `magnus` |
| `beni-tests` crate | not published | the `beni` crate's behavior suite, held outside the crate so each test reaches it through public paths alone |

Responsibility boundary: the gem stages toolchains and archives; `beni-sys` binds them. The `beni` crate is the only package consumers write Rust against. `beni-macros` is reached through it: its macros are named through `beni` and expand to code against it.

`beni-tests` ships to no one; it holds the `beni` crate's behavior from where a consumer stands. What no consumer can observe — an invariant internal to the crate — stays tested inside it.

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
    version "33.0"
    sha256 "…"
  end
end
```

#### Rakefile Settings

Relative `vendor_dir` and `build_config` paths resolve against the Rakefile's working directory. For `vendor_dir`, an explicit declaration overrides `BENI_VENDOR_DIR`, which overrides the default.

| Setting | Declared as | Default |
|---|---|---|
| `vendor_dir` | `vendor_dir <path>` — where toolchains unpack and mruby builds | `vendor/` under the Rakefile's working directory |
| `version` | `version <string>` — the mruby release version to download | `"4.0.0"` |
| `build_config` | `build_config <path>` — mruby build-config file path | undeclared — mruby's untouched upstream default config |
| targets | target declaration, optionally with a block of toolchain references | `host` when no target declaration appears |
| toolchains | toolchain reference inside a target block; toolchain definition at the top level | selection is reference-driven; every toolchain other than `mruby` defaults to its built-in pair |

Target names match the `MRuby::Build.new(<name>)` names in the config; mruby names a build defined without a name `host`. Any target declaration replaces the `host` default; the declared set is the whole set.

#### Task Outcomes

Each task leaves this state.

| Task | Outcome |
|---|---|
| `beni:build` | toolchains staged, the archive built per target |
| `beni:clean` | mruby build trees removed, vendored source kept |
| `beni:config` | self-contained, editable build config generated at the `build_config` path |
| `beni:vendor:setup` | selected toolchains downloaded and unpacked; the wasi toolchain file staged when `wasi-sdk` is selected |
| `beni:vendor:clean` | unpacked toolchains removed, tarball cache kept |
| `beni:vendor:clobber` | vendor tree removed entirely, tarball cache included |

#### Behaviors

The tasks keep these contracts.

| Behavior | Contract |
|---|---|
| Version convergence | The vendor tree converges on each toolchain's selected version: `beni:vendor:setup` replaces a staged toolchain at any other version, and `beni:build` rebuilds the archives; a stale toolchain never survives a version change. |
| Toolchain unpack | `beni:vendor:setup` unpacks from the tarball cache, downloading only the selected versions' tarballs it lacks; every unpacked tarball, cached or fresh, must match its toolchain's selected checksum. |
| Toolchain selection | The selected set is every target declaration's toolchain references plus the transitive dependencies beni resolves automatically (referencing `wasi-sdk` implies `mruby`); `mruby` is always selected. A toolchain definition selects nothing by itself; one for a toolchain nothing references is inert. |
| Build & verify | `beni:build` builds every target the build config defines, then verifies each declared target produced its compile-flags sidecar and the archive that sidecar names; a target no `target` declaration names is not verified. The config owns the target definitions; beni never reads it. |
| Staged path | Toolchains unpack at their own names under the vendor tree (the mruby source at `mruby/`); each target's archive and compile-flags sidecar stage at `mruby/build/<name>/lib/` — the staged path. |
| Archive auto-discovery | The crates auto-discover only the `host` build's archive, serving host cargo targets; any other is reachable only via `MRUBY_LIB_DIR`. |
| Compile-flags sidecar | Every build writes each archive's sidecar; it is the single ABI-alignment channel to the crates. |

#### Version Selection

`version` selects mruby; a toolchain definition never names `mruby`. Every other toolchain's selected version and checksum default to its built-in pair, and a toolchain definition replaces both. A toolchain released as one tarball per build platform downloads the build platform's tarball.

| Source | Selected checksum |
|---|---|
| mruby, default `version` | the one the installed release vendors |
| mruby, any other `version` | the pin that `version`'s first download establishes |
| built-in pair | its checksum; per-platform: the downloaded tarball's |
| toolchain definition | its single `sha256`, on every build platform |

A per-platform built-in pair vendors one checksum per tarball; a toolchain definition's `sha256` verifies only the tarball it names. A built-in pair lacking the build platform's checksum names the toolchain and platform and downloads nothing. No other platform's checksum or tarball stands in. The mruby pin persists alongside the tarball cache and shares its lifecycle; once `beni:vendor:clobber` removes both, the next download establishes a new pin.

#### Wasi Toolchain File

The wasi toolchain file carries beni's wasm32-wasip1 cross-compile settings. A re-extracted tree never lacks it, and a build config using it needs no toolchain setup of its own.

| Aspect | Contract |
|---|---|
| Written | into the staged mruby source by every `beni:vendor:setup` run selecting `wasi-sdk` |
| Activation | `conf.toolchain :wasi` inside the build config's cross-build definition |
| wasi-sdk root | `WASI_SDK_PATH` when set; otherwise the vendor tree's unpacked `wasi-sdk` |

#### Config Generation

`beni:config` seeds customization. It writes a self-contained equivalent of the configured `version`'s upstream default config to the `build_config` path.

| Property | Contract |
|---|---|
| Dependencies | requires nothing from beni at build time |
| Usability | builds without edits |
| Ownership | the consumer's; edited to define further targets, cross-compiled ones included |
| Rewrites | beni never rewrites the file |
| Parent directories | missing ones are created |
| Existing file | never overwritten; generation refuses |

### beni-sys crate — FFI surface

#### Sidecar Reading

The crate follows the `-sys` crate convention. A compile-flags sidecar flag holds only for the compiler that built the archive.

| Sidecar entry | Use |
|---|---|
| archive file name | archive looked up under it, in the directory discovery resolved |
| compiler | compiles the C shims beside the bindings |
| compile flags | given to that compiler for the C shims unchanged; binding generation takes only the declaration flags, in the form its own toolchain reads |
| libraries the archive needs linked | the ones linked, each under the name its own token carries |

Binding generation parses the headers with its own toolchain, never that compiler. It takes only the flags deciding what the headers declare: macro definitions and removals, and the language standard. The target, the sysroot, and the header tree are the crate's own.

mruby writes the sidecar in the form of the toolchain that built the archive. The archive's file name, each library's name, and each compile flag take that form; a GNU-style toolchain and MSVC differ in all three. Each token is read for what it is rather than for one toolchain's form of it. A host cargo target is served by an archive from any of mruby's host toolchains; which one built it is the archive's to state.

#### Archive Discovery

One archive serves one cargo build target. Archive discovery is environment-driven, highest precedence first. The highest-precedence variable set is the sole source, never falling back to a lower one:

1. `MRUBY_LIB_DIR` — the `-sys` crate `*_LIB_DIR` convention — names the directory containing the active target's archive and compile-flags sidecar.
2. `BENI_VENDOR_DIR` names the vendor tree the gem populated. The crate reads the `host` build's staged path, serving host cargo targets only. A cross-compiled cargo target never reads the vendor tree and requires `MRUBY_LIB_DIR`.
3. With neither variable set, the build fails, naming the variables it consults.

A documentation build runs no archive discovery, so an archive discovery variable set in one changes nothing.

#### Cross Targets

The supported cross targets are wasm32 and the other macOS architecture. A build for any other cross-compiled cargo target fails and names the unsupported target.

| Cross target | Toolchain |
|---|---|
| `x86_64-apple-darwin` from an `aarch64-apple-darwin` host, and the reverse | none — the host compiler builds either |
| wasm32 | wasi-sdk, unpacked root `WASI_SDK_PATH`, default `/opt/wasi-sdk` |

The sidecar does not record which macOS architecture an archive was built for. `MRUBY_LIB_DIR` naming the other architecture's archive resolves and fails at link.

For wasm32, the sidecar records the toolchain root the archive was built against, and the root in effect is that one. A build finding a different root, or none recorded, fails naming what it has. One root reached by two spellings is one root.

#### Version Bounds

No upper bound is declared: the FFI surface is generated from the discovered archive's own headers rather than declared per version.

| Archive | Outcome |
|---|---|
| at or above the supported mruby floor | builds |
| below the floor | the `beni-sys` build fails, named by the version its own headers record |
| headers state no version | fails the same way |
| a release the crates have not reconciled with | not refused; its changes surface as compile failures |

Such a compile failure is a symbol the wrapper calls that the archive does not declare, or a layout the crates pin.

#### Supported Configurations

The crate supports these configurations.

| Configuration | Supported |
|---|---|
| boxing | word boxing |
| language standard | any under which mruby declares its never-returning functions as never-returning |

That is the standard in effect under each of mruby's own toolchains, whether set or the compiler's default. A standard losing that form leaves those declarations in a shape the bindings do not carry, and the typed wrapper's diverging raise does not compile. Binding generation parses under the standard the sidecar names. Where it names none, the archive was built under its compiler's default, which binding generation's own toolchain need not share. Binding generation then parses under a standard that keeps the form, not under that toolchain's default.

#### Width Metadata

The crate publishes the configured integer width and the configured float width to its direct dependents, as the integer-width metadata and the float-width metadata. Both are read from the bindings the build uses: the discovered archive's own, or the documentation bindings in a documentation build.

The build fails for bindings that declare no integer width, and for bindings of an archive configured outside what the crates support:

| Unsupported archive configuration |
|---|
| built without floating point |
| a GC arena of fixed size |

#### Documentation Build

A documentation build renders the whole typed surface where no archive can be staged. Every other build generates its bindings from the discovered archive's own headers.

| Aspect | Documentation build |
|---|---|
| Bindings | the documentation bindings |
| Linking | none |
| Cargo targets | host only |
| Archive discovery | does not run; an archive discovery variable set changes nothing |

### beni crate — typed wrapper

#### Cargo features

The surface a consumer gets by default is mruby's core capability plus every
capability feature. Features fall on two axes:

| Axis | Carries | Default |
|---|---|---|
| capability feature | a capability mruby keeps in a gem rather than its core | enabled |
| dependency feature | conversions to a third-party Rust crate's types, gated as `magnus` gates the same integration | disabled |

Enabling any feature only adds surface. No combination of features removes or
replaces an item another combination carries, across the crate and inside a
type alike. A feature carries operations, never the shapes their results are
reported in. So the error a fallible operation answers has one shape in every
build, and a consumer matching on it writes the same match whatever they enabled.

##### Capability features

A consumer who never wanted the gem behind a capability feature can drop the
surface that needs it and still have a crate that builds. The feature answers whether a consumer wants the
capability, not whether the archive in front of the crate carries it:

| Rule | Effect |
|---|---|
| gem inventory | the crate reads none and adapts to no archive |
| which surface exists | the consumer's declaration settles it |
| archive lacks what the declaration names | the consumer's to reconcile |

`compiler` is a capability feature, enabled by default. It carries exactly the
function surface mruby's compiler gem defines: the compile context and the loads
that compile Ruby source into the running interpreter. Loading precompiled
bytecode needs no compiler and stays outside it. The parse message a compile
failure is reported in stays outside it too, being a result shape. Disabling
default features is how a consumer that never compiles Ruby at run time says so.

`fiber` is a capability feature, enabled by default. It carries exactly the
surface mruby's fiber gem defines: creating, resuming, testing, and yielding a
fiber, and reading the current one. The `Fiber` handle and its downcast, which
reads only the value tag, stay outside it.

##### Dependency features

A third-party trait that bounds a core capability is no dependency feature:
`bytemuck`, whose `Pod` bounds `InlineStruct`, is a dependency of every build.

`bytes` is a dependency feature carrying the `bytes` crate's `Bytes`, mirroring
`magnus`'s `RString::to_bytes` and the conversions its `bytes` feature carries:

| Operation | Effect |
|---|---|
| string handle read | reads its bytes as a `Bytes` |
| `TryConvert` | converts a String into a `Bytes` |
| `IntoValue` | copies a `Bytes` into a new String |

#### Handle, values, and conversions

The crate owns every Rust-level abstraction over the C API: an RAII interpreter
handle (`Mrb`, opened via `Mrb::open`), `Value` newtypes, and class and module
definition. Three typed conversions cross the Rust/Ruby boundary:

| Conversion | Role |
|---|---|
| `IntoValue` | out of Rust |
| `FromValue` | the downcast that reads a value only as what its type tag already is |
| `TryConvert` | the conversion a method's receiver and arguments cross, mirroring `magnus`'s `TryConvert` |

##### Type discrimination

Type discrimination is the typed handle's `FromValue` downcast, magnus's
`from_value`. Every type tag a value a typed caller holds can carry, the break
tag aside, converts into a handle. A handle accepts precisely what this table
names and rejects every other value:

| Handle | Accepts |
|---|---|
| `Qnil` / `Qtrue` / `Qfalse` | `nil` / `true` / `false` |
| `Qundef` | the undefined value no Ruby code can name; the handle never converts back into a value |
| `Integer` | an Integer, of the fixed-width or the arbitrary-width tag |
| `Float` / `Symbol` | a Float / a Symbol |
| `RString` / `RArray` / `RHash` | a String / Array / Hash, subclass instances included |
| `Range` / `Proc` | a Range / a Proc |
| `RClass` | a class or a singleton class |
| `RModule` | a module |
| `ExceptionClass` | a class descending from `Exception` — the one handle narrower than its tag |
| `Exception` | an exception object |
| `RObject` | an ordinary object — what `Object.new` allocates, and `new` on any class whose instances no other row names |
| `Fiber` / `RStruct` / `RSet` / `RComplex` / `RRational` | a Fiber / Struct / Set / Complex / Rational, from the gem defining each |
| `RTypedData` | a data carrier, holding a payload or not |
| `RInlineStruct` | an inline struct of any type |
| `RCptr` | a bare C pointer a C extension boxed |

The include-class, environment, freed-slot, and backtrace tags stay inside the
VM and carry no handle. The break tag is read through `ReprValue::as_break`. A
value answers whether it is `nil` (`ReprValue::is_nil`, as magnus's does); no
other per-type predicate exists. A class handle answers whether it names a
singleton class (`RClass::is_singleton`).

##### Numeric handle reads

Numeric handles read out as Rust numbers, mirroring `magnus`:

| Handle | Reads out as | Mirrors |
|---|---|---|
| `Integer` | `i64` | `Integer::to_i64` |
| `Float` | `f64` | `Float::to_f64` |

A fixed-width Integer always fits. An arbitrary-width Integer beyond the
configured integer width surfaces the `RangeError` mruby raises. The `Float`
read is total.

##### IntoValue rules

`IntoValue` converts a Rust value or typed handle into a `Value`. Each row is
total — cannot fail — where its last column says. The rows marked "always" also
raise nothing and run no Ruby:

| Source | Result | Total |
|---|---|---|
| `Value` | passes through unchanged | always |
| a scalar: `bool`, or a Rust integer or float the width rules admit | boxes into its Ruby value | always |
| a typed handle on a Ruby value: every Type discrimination handle except `Qundef`, and `Obj<T>` | yields that same value | always |
| `Id` | boxes into the symbol value it names | always |
| `T: TypedData` | a data carrier: a new instance of the class its type names for it, as `obj_wrap` does | for every type keeping the `TypedData` contract |
| `Inline<T>` | the handle's own value, an inline struct | for every type keeping the `InlineStruct` contract |
| a `T` the `InlineStruct` macros implement | a new instance of its type's class, as `Inline::new` does | for every type keeping the `InlineStruct` contract |
| `Bytes`, with the `bytes` feature | a copy of the bytes in a new String | — |

A Rust number type converts into an Integer or Float `Value` only where every
value it holds fits the configured integer width or configured float width, so
the conversion stays total:

| Rust type | Converts under |
|---|---|
| `i8` / `i16` / `i32` / `u8` / `u16` | every configured integer width |
| `u32` / `i64` | a 64-bit configured integer width |
| `isize` | a configured integer width no narrower than the cargo target's pointer width |
| `f32` | every configured float width |
| `f64` | a 64-bit configured float width |

Every other integer type — `u64`, `usize`, `i128`, `u128`, and a `u32` / `i64`
/ `isize` the width does not fit — has no conversion and fails to compile. So
does an `f64` under a 32-bit width.

##### FromValue rules

`FromValue` converts a `Value` by the target's rule:

| Target | Converts | Otherwise |
|---|---|---|
| `Value` | identity — the value itself | total, never rejects |
| a typed handle | what the Type discrimination table names for the handle | rejects |
| `bool` | Ruby truthiness — `nil` and `false` to `false`, every other value to `true` | total, never rejects |
| `Option<T>` | `nil` to `None`; any other value by `T`'s rule to `Some` | rejects what `T` rejects |
| `i8` / `i16` / `i32` / `i64` / `u8` / `u16` / `u32` / `u64` / `isize` / `usize` | an Integer that fits the configured integer width and lies within the target's own range | rejects |
| a Rust float | a Float | rejects |

An integer target never takes a Float. `i64` holds every Integer that fits the
configured integer width, and an unsigned target rejects every negative one. An
arbitrary-width Integer beyond the configured width rejects. Every integer
target listed converts under every configured integer width.

A float target never takes an Integer. It converts only where it holds every
value the configured float width does: `f64` under every width, `f32` under a
32-bit width. An `f32` under a 64-bit width has no conversion and fails to
compile.

##### TryConvert contract

`TryConvert` answers the converted value, or an `Err` carrying the exception
mruby raises for the same mismatch, worded as mruby words it. It runs no user
Ruby. Each rule names what converts. Every other value surfaces the `TypeError`
"*value* cannot be converted to *target*", unless the rule names another error:

| Placeholder | Names |
|---|---|
| *value* | the value's class — or `nil`, `true`, or `false` itself |
| *target* | the Ruby class the rule converts from |

##### TryConvert handles

The handle targets below convert what the `FromValue` downcast converts and nothing more:

| Target | Converts | Error for any other value |
|---|---|---|
| `Value` / `bool` / `Option<T>` | as the `FromValue` rule for each | `Option<T>` answers `T`'s `Err` where `T` rejects |
| `RString` / `RArray` / `RHash` / `Symbol` / `Range` | downcast only | default `TypeError`; *target* is the handle's Ruby class |
| `RClass` / `RModule` / `ExceptionClass` | downcast only | `TypeError` "*value* is not a class", "*value* is not a module", or "*value* is not a class inheriting Exception" |
| `Proc` | a `Proc` alone | `TypeError` "wrong argument type *class* (expected Proc)" |
| `Qnil` / `Qtrue` / `Qfalse` / `Exception` / `RObject` / `Fiber` / `RStruct` / `RSet` / `RRational` / `RComplex` / `RInlineStruct` / `RCptr` | downcast only | `TypeError` "wrong argument type *value* (expected *name*)" |

mruby has no implicit `to_str`, `to_ary`, or `to_hash` conversion. A class,
module, or exception-class handle's *value* is the inspected form, as mruby
words a class or module mismatch. `Proc` dispatches no `to_proc`; mruby
converts to a `Proc` only a block being passed. An `Exception` handle dispatches
no `exception`. `Qundef` has no `TryConvert`.

In "wrong argument type *value* (expected *name*)", *value* is named as mruby's
own type check names it. *name* is the name that check gives the handle's type
tag — `Exception`, `Object`, `Fiber`, `Struct`, `Set`, `Rational`, `Complex`,
`istruct`, or `cptr` — and, for `Qnil` / `Qtrue` / `Qfalse`, `NilClass` /
`TrueClass` / `FalseClass`.

##### TryConvert numbers

Rust integer and float targets convert as mruby's own C-method arguments do; the `Integer` and `Float` handles convert as the paragraph after this table says:

| Target | Converts | *target* |
|---|---|---|
| `i8` / `i16` / `i32` / `i64` / `i128` / `u8` / `u16` / `u32` / `u64` / `u128` / `isize` / `usize` | an Integer, or a Float truncated toward zero | `Integer` |
| a non-zero Rust integer, `NonZeroI8` … `NonZeroUsize` | as its integer does | as its integer |
| `f64` / `f32` | a Float, or an Integer widened | `Float` |
| `Integer` handle | the downcast, a Float truncated toward zero, an arbitrary-width Integer kept whole | `Integer` |
| `Float` handle | the downcast, an Integer widened | `Float` |

The `Integer` and `Float` handles take any non-downcast numeric value as the
number mruby's own numeric coercion yields. This mirrors `magnus`'s conversions
through `to_int` and `to_f` without dispatching either. `f64` and `f32` convert
under every configured float width. An `f32` narrows a wider Float to its
nearest `f32`, and a magnitude beyond `f32` becomes an infinity.

| Target | Value | Error |
|---|---|---|
| Rust integer, `Integer` handle | an infinite or NaN Float | the `RangeError` mruby raises |
| Rust integer | an arbitrary-width Integer beyond the configured integer width | the `RangeError` mruby raises |
| Rust integer | an Integer within that width, outside the target's own range | `RangeError` "*value* out of range", *value* its inspected form |
| non-zero Rust integer | zero | `ArgumentError` "value must be non-zero" |
| `f64` / `f32`, `Float` handle | `nil` | `TypeError` "can't convert nil into Float", as mruby raises |

##### TryConvert text and collections

Text targets convert a String's bytes; sequence and map targets convert each
element by its own type's rule:

| Target | Converts | *target* |
|---|---|---|
| `String` | UTF-8 bytes | `String` |
| `char` | UTF-8 bytes holding exactly one character | `String` |
| `PathBuf` | any bytes on a Unix target, UTF-8 bytes on any other | `String` |
| `Bytes`, with the `bytes` feature | any bytes | `String` |
| `Vec<T>` / `[T; N]` / a tuple of 1 to 12 elements | an Array | `Array` |
| `HashMap<K, V>` / `BTreeMap<K, V>` | a Hash, each key and value | `Hash` |

Bytes that are not UTF-8 where UTF-8 is required surface the `ArgumentError`
"invalid UTF-8 byte sequence". For `char`, a string of any other length than one
character surfaces the `TypeError` with *target* `char`. No path protocol
applies — mruby has no `to_path`.

A sequence surfaces the first element's `Err`, a map the first `Err`. A
fixed-length target converts an Array of exactly its length, and surfaces the
`TypeError` "expected Array of length *N*" for any other. A sequence or map
target holds any element type `TryConvert` converts to, a `Value` or typed
handle included. Each element crosses out to Rust and stays reachable as the
Garbage collection section promises for every value that does, wherever the
Rust side stores it.

##### TryConvert data

Data targets convert a data carrier or an inline struct:

| Target | Converts | Result |
|---|---|---|
| `RTypedData` | any data carrier, holding a payload or not | typed handle |
| `&T` / `Obj<T>` for `T: TypedData` | a data carrier holding a payload of `T`'s data type | reference to the payload, or typed handle |
| `Inline<T>`, or a `T` the `InlineStruct` macros implement, for `T: InlineStruct` | an inline struct whose class belongs to `T` | typed handle, or a copy of the payload |

Every other value surfaces a `TypeError`. For `&T` / `Obj<T>` it is the one
mruby's own data-type check raises:

| Target | Value | `TypeError` message |
|---|---|---|
| `RTypedData`, `&T` / `Obj<T>` | no data carrier | "wrong argument type *value* (expected C data)" |
| `&T` / `Obj<T>` | a carrier of another data type | "wrong argument type *name* (expected *T name*)" |
| `&T` / `Obj<T>` | a carrier holding no payload | "uninitialized *class* (expected *T name*)" |
| `Inline<T>` / `T: InlineStruct` | any other value | "wrong argument type *value* (expected *T name*)" |

For `RTypedData` and the inline struct targets, *value* is named as mruby's own
type check names it. *name* is the other data type's name and *class* the
carrier's class. *T name* is the name `T`'s data type declares, or for an
inline struct target the name `T` declares. The reference borrows the payload
for as long as its carrier stays reachable.

##### Numeric quantities

Integer and float quantities cross the typed surface as Rust's own types, never
as the configured-width integer or float the raw bindings declare:

| Quantity | Rust type |
|---|---|
| an index or offset mruby counts back from the end when it is negative | `isize` |
| every other count, length, or offset — one mruby never computes with as a negative value, whether it rejects a negative one or cannot be handed one | `usize` |
| an integer mruby produces — an Integer's value, a parsed integer, an object identifier | `i64`, holding every configured integer width |
| a float mruby produces — a Float's value, a parsed float, a numeric conversion's result | `f64`, holding every configured float width |

So a signature reads the same under every configured integer width and every
configured float width. An index, offset, or length beyond what the configured
integer width holds stands for the width's minimum or maximum it exceeds, so it
never wraps onto an in-range position. The crate takes each configured width
from its own metadata alone.

#### Strings

Rust bytes convert to a new mruby string, returned as a typed `RString`. A
string constructs three ways:

| Construction | Result |
|---|---|
| copying | a new string owning a copy of Rust bytes |
| with capacity | an empty string with a preallocated buffer, as Ruby's `String.new(capacity:)` reserves for appends that follow |
| borrowed static | a string aliasing a borrowed static buffer, without copying its bytes |

The borrowed static construction is the no-copy counterpart of the copying
conversion. The buffer must stay valid for the whole run of the program, since
mruby never frees it. The construction enforces this as a `'static` requirement,
making a dangling alias impossible. mruby treats such a string copy-on-write: an
in-place append or resize reallocates first, then behaves like any other string.
magnus has no direct analogue, so this construction anchors on mruby's own
`mrb_str_new_static`, with `mrb_str_new_lit` the convenience that borrows a
string literal.

##### Byte Reads

Rust reads an mruby string's bytes these ways. The reads below never raise.

| Read | Yields | Rejects |
|---|---|---|
| borrowed slice | a byte view of the string | — |
| owned `String` | the bytes when valid UTF-8 | a non-string tag, or non-UTF-8 bytes |
| owned `Vec<u8>` | arbitrary bytes | a non-string tag |
| owned `Bytes`, read from a string handle with the `bytes` feature | arbitrary bytes | — |

##### Fallible Reads

A string handle also reads its bytes in ways that surface an `Err`:

| Read | Yields | `Err` when |
|---|---|---|
| owned `String` | the bytes | bytes not UTF-8: the `ArgumentError` "invalid UTF-8 byte sequence" |
| `char` | the one character | bytes not UTF-8: the same `Err`; other than exactly one character: a `TypeError` |
| NUL-terminated C-string view | the bytes, guaranteed to end in a `\0`, suitable for a C boundary | an embedded NUL: the `ArgumentError` mruby raises |

The `String` and `char` reads mirror `magnus`'s `RString::to_string` and
`to_char`, the reads the `String` and `char` `TryConvert` rules make. A C string
cannot carry an embedded NUL. magnus offers no direct C-string accessor, so that
read anchors on mruby's own `mrb_string_cstr`.

##### Numeric Parses

Rust also parses a string's bytes to a number. Each `Err` carries the
`ArgumentError` mruby raises.

| Parse | Yields | `Err` when | Anchor |
|---|---|---|---|
| strict integer | the integer in a given radix | bytes not a valid integer in that radix; invalid radix | `mrb_str_to_integer` |
| lenient integer | the leading integer, ignoring trailing characters; `0` when no integer begins the bytes | invalid radix only | `mrb_str_to_inum` |
| strict float | the float | bytes not a valid float | `mrb_str_to_dbl` |

The radix is one of 2 through 36, or 0 to auto-detect a leading base prefix
(`0x`, `0b`, `0o`) — the radixes Ruby's `String#to_i` accepts. A radix outside
that domain is invalid input for both integer parses. The lenient parse cannot
interpret it either, and surfaces an `Err` carrying the same `ArgumentError`.

The strict integer parse rejects any non-integer input rather than stopping at
the first invalid character. It is the strict counterpart of Ruby's lenient
`String#to_i`, which never raises. The lenient parse reads the way `String#to_i`
itself does. It returns the best-effort value directly, without an `Err` for
malformed content. The strict float parse rejects any non-float input rather
than ignoring trailing characters. It is the strict counterpart of Ruby's lenient
`String#to_f`, which never raises.

##### Numeric Conversions

An `Integer` handle and a `Float` handle convert as follows.

| Conversion | Example | `Err` when | Mirrors |
|---|---|---|---|
| `Integer` to a new `RString` in a given radix, as Ruby's `Integer#to_s(base)` | `12345` to `"3039"` in base 16 | radix outside 2 through 36: the `ArgumentError` mruby raises | `mrb_integer_to_str` |
| `Float` to the `Integer` it truncates toward zero, as Ruby's `Float#to_i` | `3.9` to `3`, `-3.9` to `-3` | infinite or NaN float, which has no integer: a `RangeError` | `mrb_float_to_integer` |

##### Numeric Arithmetic

Two numeric values add, subtract, or multiply into a new numeric value, the way
Ruby's `+`, `-`, and `*` do on `Integer` and `Float` — `2 + 3` to `5`, `2 + 3.5`
to `5.5`. The result stays in mruby's value domain:

| Operands | Result |
|---|---|
| both integers, result fits the configured integer width | an `Integer` |
| either operand a float | a `Float`; the integer operand widens |
| integer arithmetic exceeding the configured integer width | an `Err` carrying a `RangeError` |
| non-numeric left operand | an `Err` carrying a `TypeError` |
| non-numeric right operand | an `Err` carrying a `TypeError` |

Each operation dispatches its receiver on the numeric tag rather than trusting
it. magnus offers no mruby-native arithmetic — its `coerce_bin` routes through
the full Ruby coercion protocol with no mruby counterpart. These operations
anchor on mruby's own `mrb_num_add` / `mrb_num_sub` / `mrb_num_mul`.

##### String Appends

A registered method grows an `RString` in place, the way Ruby's `String#<<`
extends its receiver, by appending:

| Append | Source |
|---|---|
| bytes | Rust bytes |
| string | another mruby string's bytes |
| C string | a NUL-terminated C string's content up to the terminating NUL |
| coerced value | any value coerced to a string, as Ruby's `String#concat` accepts a non-string argument |

The C-string append is the C-boundary counterpart of the byte append, anchored on
mruby's own `mrb_str_cat_cstr`. The coerced append is the dispatching
counterpart to the byte and string appends.

##### String Operations

Beyond reading and appending, a string offers these operations:

| Operation | Ruby | Result |
|---|---|---|
| duplicate | `String#dup` | an independent copy |
| frozen copy | — | a frozen string holding the same bytes, mirroring `magnus`'s `RString::new_frozen`: the string itself when it is already frozen, otherwise a new copy that leaves the original unfrozen |
| byte equality against another | `String#==` | total read; dispatches nothing, never raises |
| byte-content order against another | `String#<=>` | total read; dispatches nothing, never raises |
| intern own bytes | `String#intern` | the typed `Symbol` they name |
| concatenate with another string | `String#+` | a new string |

The intern creates the symbol when it does not yet exist, and dispatches nothing.
Like every creating intern, it surfaces an `Err` for bytes too long to name a
symbol.

The concatenation anchors on mruby's own `mrb_str_plus`. The result is a freshly
allocated string holding both operands' bytes, and neither operand is mutated.
It is the non-mutating counterpart of the in-place append, which grows its
receiver. With both operands already strings, it dispatches nothing and never
raises, returning the new string directly rather than a fallible result.

##### Resize and Search

A registered method also resizes and reads a string:

| Operation | Result |
|---|---|
| resize length in place | truncates, or extends with undefined trailing bytes |
| substring by character range | the substring, or nothing when the range falls outside the string |
| search for a substring | the first match's byte index at or after a start offset; nothing when absent |

The search is a total read that dispatches nothing and never raises. A negative
offset counts from the string's end. An offset past the end finds nothing. An
empty substring is found at the offset itself.

#### Symbols

An interned id crosses the typed surface as the typed `Id`, and a symbol value
as the typed `Symbol` — `magnus`'s split:

| Type | Carries |
|---|---|
| `Id` | the id itself, which is not a value |
| `Symbol` | the handle on the symbol value that boxes the id |

Each converts into the other safely. The conversion dispatches nothing, never
raises, and needs no interpreter. A signature taking or yielding a name reads
the same whatever the raw bindings call the interned id.

`Id` is where the raw id itself crosses: a raw id reifies into an `Id`, and an
`Id` reads its raw id back out. This is the seam a consumer working below the
typed surface hands an id to `beni::sys` through and takes one back from.
Reading the id out is safe. Reifying one is `unsafe`, like every crossing into
the typed domain. A wrongly reified id has a concrete cost: the name reification
below answers an id naming no symbol with a value carrying no String, and hands
that back as one.

##### Name Interning

The Rust-side name reaches the intern in these forms:

| Name form | Interns |
|---|---|
| NUL-terminated C string | the bytes up to the first NUL |
| borrowed byte slice carried with its own length | the exact bytes the slice spans |
| mruby String value | its bytes |
| borrowed static buffer | the caller's bytes, aliased without copying |

The length-carrying byte slice is the general form. A name that embeds a NUL or
is not NUL-terminated interns whole, where the C-string form would stop at the
first NUL.

The static intern is the no-copy counterpart of the copying interns: the
interned name aliases the caller's bytes instead of owning a copy. The borrowed
buffer must stay valid for the whole run of the program, a `'static` requirement
the intern enforces, since mruby keeps the pointer and never frees it. This
intern anchors on mruby's own `mrb_intern_static`, with `mrb_intern_lit` the
convenience that borrows a string literal.

##### Intern Outcomes

The interns and the existence checks behave as follows. None dispatches.

| Operation | Yields | `Err` |
|---|---|---|
| intern | the `Id` the name interns to, creating the symbol when none exists yet | the `ArgumentError` mruby raises for a name of `UINT16_MAX` bytes or more |
| id existence check | the `Id` the name already interns to, or nothing | never raises |
| symbol existence check | the `Symbol` boxing the `Id` the name already interns to, or nothing | never raises |

The interns mirror `magnus`'s `intern`, and the existence checks its `check_id`
and `check_symbol`. A name of `UINT16_MAX` bytes or more is too long to be a
symbol; every shorter name interns. An existence check takes the name as a
length-carrying byte slice and never creates a symbol. It answers nothing for a
name not yet interned, and nothing for a name too long to be a symbol.

##### Id Equality

An `Id` and a `Symbol` compare and hash as follows — the equality and hashing
`magnus` gives the two:

| Pair | Compares by | Hashing |
|---|---|---|
| `Id` with `Id` | the id it is | hashes by the id; keys a Rust-side map |
| `Symbol` with `Symbol` | the id it boxes | keys no map itself |
| `Symbol` with `Id` | the id | — |

Interning is canonical, so two ids are equal exactly when they name the same
bytes. All of them are total, dispatch nothing, and never raise.

##### Value Coercion

Where the interns take Rust bytes, an existing mruby value also coerces into a
typed `Symbol`:

| Value | Result |
|---|---|
| symbol | its own id |
| string | its contents interned; the creating interns' `Err` when too long to be a symbol |
| any other | an `Err`: the `TypeError` mruby raises for a non-symbol, non-string |

The coercion dispatches no user Ruby. It follows the raise/return contract like
the other converting operations.

##### Name Reads

Beyond the id, a symbol reads its name three ways, each copying the name out
into an owned Rust value. All are non-dispatching reads that never raise, each
yielding nothing when mruby has no name for the id.

| Read | Yields |
|---|---|
| name as owned UTF-8 string | the UTF-8 name, escaped to its quoted dump form when it embeds a NUL |
| name as owned bytes | the raw name bytes with their true length, embedded NUL bytes included and unescaped |
| dump form | the name's symbol-literal representation, quoted and escaped when the name is not a plain identifier — Ruby's `Symbol#inspect` without the leading colon |

mruby has no storage a short symbol name can be borrowed from stably: a short
name unpacks into a buffer that the next name read overwrites. The typed surface
therefore copies the name out rather than aliasing it.

A symbol also reifies its name as an mruby String value, the way Ruby's
`Symbol#to_s` does. Unlike the three reads, it produces a distinct, mutable
mruby `RString` value whose bytes are the symbol's name — unfrozen, unlike
`Symbol#name`. Like the others, it dispatches nothing and never raises.

#### Ranges

A Range constructs and reads its parts as follows:

| Operation | Result |
|---|---|
| construct from begin value, end value, exclusive-end flag | a Range, mirroring Ruby's `Range.new(begin, end, exclusive)` |
| construct with bounds that cannot be compared | an `Err` — the `ArgumentError` mruby raises for a bad range |
| read begin value, end value, or whether it excludes its end | three non-dispatching reads that never raise |

##### Slice Computation

Given a collection length, a Range computes the normalized slice it covers of a
collection that long. This is the primitive behind slicing a collection by a
Range, the way Ruby's `Array#[range]` / `String#[range]` resolve a Range index.
The caller distinguishes three outcomes:

| Outcome | When | Carries |
|---|---|---|
| in-range | — | a begin offset and a selected length, both non-negative |
| out-of-range | the begin offset falls before the collection start | no offsets |
| mismatch | the receiver is not a Range | no offsets |

The begin offset and selected length are meaningful only on the in-range
outcome. Bounds resolve as follows:

| Bound | Treatment |
|---|---|
| negative begin or end | counts back from the collection length |
| missing begin or end | stands in for the collection's first or last index |
| end before the begin | selects a length of zero |

A truncation flag governs the over-long range:

| Flag | Over-long range |
|---|---|
| set | a begin offset past the collection length is also out-of-range; an end past the length is clamped to it |
| clear | the end is taken as given; a slice running past the collection length is reported in-range with whatever length the bounds yield |

##### Slice Errors

The computation runs no user Ruby and does not dispatch. The three outcomes are
returns, not errors:

| Case | Result |
|---|---|
| a present (non-missing) bound neither an integer nor integer-convertible | an `Err` carrying a `TypeError` — the same `TypeError` mruby's implicit integer coercion raises |
| any other input | one of the three outcomes |

magnus exposes the same primitive as its `Range::beg_len`, which collapses
out-of-range and mismatch into one `Err` because CRuby's own primitive raises on
both. mruby returns the three-way outcome instead, and the typed surface
preserves all three.

#### Errors and the raise/return contract

A registered method raises its own exception by building one and returning it as an `Err`. The `Err` reaches the method's Ruby caller as an mruby exception like any other `Err`.

| Built from | Message |
|---|---|
| exception-class handle and Rust bytes | the bytes, copied into a fresh string |
| exception-class handle and an existing mruby `RString` value | the value, carried as-is without a Rust-side copy |
| a given count and the expected minimum and maximum, when validating its own argument count | the canonical `ArgumentError` ("wrong number of arguments (given N, expected …)") mruby itself produces |

Building the exception neither raises nor runs user Ruby. The handle names a class whose instances are exceptions, and the `RString`-valued form is statically a string, so neither has a type to reject.

##### Exception matching

An `Err` answers whether the exception it carries is an instance of a given class or module, walking the ancestry as Ruby's `is_a?` does.

| `Err` carries | Answer |
|---|---|
| an exception | Ruby's `is_a?` against the class or module |
| a parse failure, a panic, or a break object | no — it carries no exception |

Rescuing by class is a `match` on that answer, and cleanup is the code after the operation. So the typed surface carries no `begin`/`rescue` or `begin`/`ensure` combinator.

##### Raw helpers

`beni::sys` carries every raw binding and the conversions between a typed form and its raw one. These are the safe read of a value or interned id out of it, and the `unsafe` crossing back in. It also carries two helpers for code working below the typed surface:

| Helper | Behavior |
|---|---|
| `sys::protect` | runs a body inside mruby's protected frame and answers its value |
| `sys::catch_unwind` | surfaces a panic in its closure as an `Err` carrying the panic's message |

Under `sys::protect`, an exception a raw binding raises surfaces as an `Err` carrying it. The raise leaves the frames it crosses without returning through them. Whether their destructors run on the way is not guaranteed. So the code making that raw call keeps its own frames free of anything that could need dropping. A panic in the body aborts the process.

`sys::catch_unwind` is the boundary a Rust closure handed to mruby as a C callback needs.

##### Contract table

Every mutating or dispatching operation across the typed surface follows one raise/return contract:

| Operation kind | Surfaces `Err` | Returns |
|---|---|---|
| Mutates a receiver: array append / remove / extend / replace / clear; array indexed write and resize; hash assign / delete / merge / clear; string append and resize; instance-variable assignment and removal; class-variable assignment; constant assignment and removal | the receiver is frozen; an indexed write also when the index is out of range (a negative index past the beginning, or one too large); a string resize also when the requested length is negative or overflows | `Result` |
| Dispatches Ruby: a method call; `==` / `eql?`; a `<=>` comparison; an object `dup`; string coercion; a splat coercion to an array running a non-array's `to_a`; an array join rendering each element via `to_s`; an instance construction running `initialize`; a constant fetch running a `const_missing` hook; a constant assignment running a `const_added` hook; a hash read / assignment / fetch / key test / deletion / merge running a key's `hash` / `eql?`; a hash read running a `default` lookup for an absent key; a range construction comparing its two bounds; a current-fiber read running `Fiber.current` | the dispatched code raises; a splat coercion also when a `to_a` responder returns a non-array non-`nil` value; a constant fetch also when the name resolves to no constant; a range construction also when its two bounds cannot be compared; a current-fiber read also when `Fiber` names no class, or `Fiber.current` answers anything but a Fiber (`TypeError`) | `Result`; a `<=>` comparison yields nothing when the two values are incomparable |
| Reads a named variable that raises on absence: a class-variable read, walking the ancestry | the name resolves to no class variable | `Result` |
| Converts or computes without dispatching: a `TryConvert` conversion; an instance-variable read converted to a requested type; an Integer read out as `i64`; a Float to the Integer it truncates; an arithmetic (add / subtract / multiply) of two numeric values | a `TryConvert` mismatch, as its rule names; an arbitrary-width Integer beyond the configured integer width read out (`RangeError`); either arithmetic operand is non-numeric (`TypeError`); an infinite / NaN float converts to integer (`RangeError`); an integer arithmetic exceeds the configured integer width (`RangeError`) | `Result` |
| Interns a name, creating its symbol: a C-string, byte-slice, String-value, or static-buffer intern; a string interning its own bytes | the name is `UINT16_MAX` bytes or longer (`ArgumentError`) | `Result` |
| Reads or renders without dispatching but can still raise: a string's NUL-terminated C-string view; a strict parse of a string to an integer in a given radix, or to a float; rendering an integer to a string in a given radix; computing a Range's normalized slice of a collection length; reading an instance-variable holder's singleton class | the bytes contain an embedded NUL; the bytes are not a valid integer in the radix; the bytes are not a valid float; the render radix is outside 2 through 36; a Range slice's present bound is neither an integer nor integer-convertible (`TypeError`); mruby gives the object no singleton class (`TypeError`) | `Result`; a Range slice that does not raise returns its three-way outcome: in-range with begin offset and length, out-of-range, or a non-Range mismatch |
| Marks a class so its instances carry Rust data; prepares a `TypedData` type's carrier classes in an interpreter | the class's instances are neither plain objects nor data carriers: a singleton class, or a class whose instances have a built-in layout such as an exception, a string, or a number (`TypeError`); preparing also when a class path resolves to no class, or to a value that is not a class | `Result` |
| Reads a data carrier's payload through an `RTypedData` handle; copies a carrier through `typed_data::Dup`'s `clone` | the carrier holds no payload, or one of another data type (`TypeError`); `clone` also when it is passed an argument (`ArgumentError`), the copy's `initialize_copy` raises, or that `initialize_copy` installs a payload of another data type (`TypeError`) | `Result` |
| Prepares an `InlineStruct` type's class in an interpreter; converts a value to an inline struct of a type; replaces an inline struct's payload | preparing: the class's instances are not plain objects and the class does not belong to the same type (`TypeError`), or the class path resolves to no class or to a value that is not a class; converting: the value is no inline struct of the type (`TypeError`); replacing: the receiver is frozen (`FrozenError`) | `Result` |
| Reads the call's arguments by shape: a scan read or the single-argument read in a method registered for any arity; a named keyword read of a keyword hash | the call does not fit the read's shape: too few or too many positionals; an argument or keyword value of the wrong type; a missing required block; a missing required keyword; a keyword no list names when the read collects no rest. A scan read of an array-handle splat and an optional block alone fits every call | `Result` |
| Compiles and runs Ruby source, under a caller's compile context or one borrowed for the load | the source does not parse; the context's filename is too long to be a symbol; a codegen step fails; the program raises while it runs | `Result`; a parse failure carries a parse message, every other failure carries the exception |
| Switches fibers: a fiber creation, resume, or alive test; a registered method's returned fiber yield | creation: the `Proc` is backed by a C function; resume: the fiber has finished, is the running fiber or one already resumed, was transferred to, or was never initialized, or its block raises; alive test: the fiber was never initialized; fiber yield: the method runs outside a resumed fiber, or a call from C or Rust code into Ruby stands between the fiber's block and the method | `Result`; a fiber yield's `Err` raised to the method's Ruby caller |
| Reads or examines without dispatching: indexed read; an array's borrowed slice; keys; values; size; emptiness; container duplication; substring read by character range; substring search by byte index; byte comparison; symbol name and dump reads; range begin / end / exclusive-end reads; instance-variable, class-variable, and constant presence; `respond_to?`; `equal?`; `is_a?`; `instance_of?`; class; type downcast; `nil` test | never | a bare value, or the absent value when the substring range or an absent symbol name falls outside the read |

##### Warnings and bugs

Two calls report to the process's standard error rather than to a caller:

| Call | Behavior |
|---|---|
| warn, on the `Mrb` handle | writes `warning: `, the message's bytes whole, and a newline, mirroring mruby's `mrb_warn`; a message longer than the longest string the interpreter holds writes nothing and answers the `Err` carrying mruby's `ArgumentError`; an archive built without standard I/O writes nothing and answers `Ok` for every message |
| bug, in `beni::error` | writes `bug: `, the message, and a newline, then ends the process with a failure status, mirroring `magnus`'s `error::bug`; it never returns; a message holding a NUL is written as `panic`; an archive built without standard I/O writes nothing and still ends the process |

warn writes on every call: mruby keeps no verbose switch, where `magnus`'s `Ruby::warning` writes only when Ruby runs verbose.

#### Containers

The typed array carries Ruby `Array`'s surface:

| Operation | Behavior |
|---|---|
| construct | empty, with a preallocated capacity, from a slice of values, or as a pair of two given values |
| append | add a value to the end |
| indexed read | the element, or `nil` when the index is out of range |
| borrowed slice | the elements in place, mirroring `magnus`'s `unsafe` `RArray::as_slice`; the view holds until a call that could change or move them |
| index walk | visit the elements from first to last |
| indexed write | Ruby's `ary[i] = v`, growing with `nil` to reach past the end |
| resize | set the length: grow with `nil` to a longer length, truncate to a shorter one |
| remove | take a value from either end |
| extend | append another array's elements |
| replace | mutate the receiver in place to hold a copy of another array's elements, not a new array |
| splice | replace a subsequence in place |
| join | render the elements into one string |
| clear | empty it |
| duplicate | copy it |
| convert to a Rust sequence | each element through `TryConvert` |

##### Index walk

The walk reads each element as the indexed read does — the element, or `nil`. It dispatches no Ruby `#each` and runs over a length fixed when the walk begins. It is a live walk, not a content snapshot:

| Position during the walk | Read |
|---|---|
| an element appended past that length | not visited |
| a position the array no longer reaches | `nil` |
| a position whose element changed | its current value |

Capturing the elements as they stand requires duplicating the array first. The walk dispatches no Ruby and surfaces no `Err`.

##### Array splice

Splice is Ruby's `ary[head, len] = rpl`: it removes the `len` elements starting at `head` and puts the replacement in their place. It is the primitive behind indexed assignment, insertion, and deletion.

| Case | Behavior |
|---|---|
| `head` past the end | grows the array with `nil` to reach it |
| negative `head` | counts from the tail |
| array replacement | splices in its elements |
| any other replacement value | inserted as the single element it is |
| absent replacement | deletes without inserting |
| `len` overshoots the tail | the available run is truncated |
| receiver frozen | `Err` |
| `head` reaches past the beginning | `Err` |
| `head` and `len` together overshoot the maximum array size | `Err` |

##### Join and conversion

Join renders the elements into one string, separated by a given separator. Each element's `to_s` runs, and a raise inside it surfaces as an `Err`. An absent separator concatenates the renderings with nothing between them.

Conversion to a Rust sequence converts each element through `TryConvert`, mirroring `magnus`'s `RArray::to_vec` and `to_array`:

| Target | Condition | `Err` surfaced |
|---|---|---|
| a Rust vector | any length | the first element's `Err` |
| a fixed-length Rust array | the array holds exactly that many elements | the first element's `Err` |
| a fixed-length Rust array | any other length | `TypeError` "expected Array of length *N*" |

##### Hash operations

A typed hash constructs empty, or empty with a preallocated capacity. The capacity reserves room for the assignments that follow; it is a hint, not content, and the hash starts empty. Beyond construction it carries Ruby `Hash`'s surface:

| Operation | Behavior |
|---|---|
| assign | set a key's value |
| read | the value, or `nil` when the key is absent; `Err` when a key's `hash` / `eql?` or an absent-key `default` lookup raises |
| fetch | the value, or a supplied default when the key is absent, like Ruby's `Hash#fetch(key, default)` |
| key test | whether a key is present |
| delete | remove a key, returning its former value |
| merge | fold another hash into this one |
| clear | empty it |
| duplicate | copy it |
| keys / values | read as typed arrays |
| size / emptiness | the entry count, and whether it holds no entries |
| convert to a Rust map | each key and value through `TryConvert`, as a Rust hash map or ordered map; surfaces the first `Err` |
| iterate | visit each key-value pair in insertion order |

The Rust map conversion mirrors `magnus`'s `RHash::to_hash_map` and `to_btree_map`.

##### Hash iteration

Iterate hands each key-value pair, in insertion order, to a closure that signals whether to continue or stop. It returns a `Result`. The walk dispatches no Ruby of its own.

| Closure action | Outcome |
|---|---|
| stops | the walk ends before the remaining pairs |
| re-enters the VM to mutate the hash's table | `Err` carrying the `RuntimeError` mruby raises for the in-walk modification |
| panics | the walk stops; the panic resurfaces on the Rust side once the walk unwinds, never crossing into mruby's frames |

#### Value operations

Each operation is reached through the handle its receiver must be, as `magnus` places it on `ReprValue`, `Object`, or `Module`:

| Receiver | Operations |
|---|---|
| any handle | every operation the next two rows do not name |
| instance-variable holder | instance variable, singleton class |
| class or module handle | class variable, constant |

##### Identity and type

These operations test identity, equality, order, and type:

| Operation | Semantics |
|---|---|
| `equal?` | object identity — the same object or not; a total predicate |
| `object_id` | a unique integer identifier for the value; dispatches nothing, never raises; total |
| `==` / `eql?` | Ruby value and hash-key equality; may run a user-defined `==` or `eql?` |
| comparison | three-way order by Ruby's `<=>`: less, equal, or greater |
| `is_a?` | an instance of a given class or module, walking the ancestry as Ruby's `is_a?` does |
| `instance_of?` | a direct instance of a given class or module |
| class | the class the value belongs to |
| `respond_to?` | whether the value answers to a named method; a total predicate |

A comparison yields nothing when the two values are incomparable. It may run a user-defined `<=>`, and a raise inside it surfaces as an `Err`. It is distinct from equality: it ranks rather than tests sameness.

For `instance_of?`, only the class the value belongs to matches, so neither a superclass nor a module ever does.

##### Dispatch and rendering

These operations call, render, copy, or freeze a value:

| Operation | Semantics |
|---|---|
| dispatch | call a Ruby method named by a symbol-or-name key with an argument slice, receiving its return value |
| inspect | the value's debug string, Ruby's `inspect`; runs a user-defined `inspect` |
| default render | the value's default `to_s` form as a new string; runs no user Ruby and is total |
| `dup` | copy the object, running its `initialize_copy`; may raise |
| string coercion | itself when already a string, otherwise its `to_s` |
| freeze | freeze the value in place |
| frozen check | `Err` when the value is frozen, `Ok` otherwise; runs no user Ruby |

Dispatch can also pass an explicit block — a typed `Proc` the method yields to — alongside the argument slice. The plain dispatch is the no-block call; a caller wanting no block uses it rather than passing a nil block.

A raise inside `inspect` yields an empty string. Default render gives `#<ClassName>` for an immediate and `#<ClassName:0x...>` for a heap object. It builds from the class name without dispatching the value's own `to_s`, unlike string coercion, which runs the receiver's `to_s`.

A `dup` copy is unfrozen and carries no singleton class; an immediate returns itself. String coercion may raise when `to_s` does not return a string. The frozen check is a precondition guard, and an immediate counts as frozen.

##### Arithmetic

Arithmetic adds, subtracts, or multiplies two numeric values into a new numeric value — Ruby's `+` / `-` / `*` on `Integer` and `Float`. It stays in mruby's value domain and runs no user Ruby.

| Operands | Result |
|---|---|
| both integers, result fits the configured integer width | an `Integer` |
| either operand a float | a `Float` |
| either operand non-numeric | `Err` carrying a `TypeError` |
| integer arithmetic exceeds the configured integer width | `Err` carrying a `RangeError` |

##### Splat coercion

Splat coercion spreads the value to a new typed `RArray`, Ruby's `*` coercion:

| Value | Result |
|---|---|
| an array | a copy of itself |
| a non-array whose `to_a` returns an array | that array |
| a non-array whose `to_a` returns `nil` | the value wrapped in a one-element array |
| a value that answers no `to_a` | the value wrapped in a one-element array |
| a non-array whose `to_a` raises or returns a non-array non-`nil` value | `Err` |

The tag-coercion to an `RArray` handle dispatches nothing and takes only an already-array-tagged value. Splat coercion instead runs `to_a` and always yields an array.

##### Singleton class

The singleton class operation reads the holder's own singleton class, Ruby's `singleton_class`:

| Property | Behavior |
|---|---|
| holds | methods defined on that one object — the per-instance eigenclass |
| differs from | the regular class the holder shares with its peers |
| creation | on first read; stable across re-reads of the same object |
| failure | `Err` carrying a `TypeError` only where mruby gives the object no singleton class |

It runs no user Ruby.

##### Instance variables

A holder's named instance variables support these operations:

| Operation | Behavior | Surfaces `Err` |
|---|---|---|
| read | through `TryConvert` into a requested type; `nil` when unset | only its key's or its conversion's `Err` |
| assign | any `IntoValue` value, in place | when the holder is frozen |
| presence test | whether it is set | never raises |
| remove | yields the former value; distinguishes an absent variable from one removed while holding `nil` | only when the holder is frozen |
| iterate | every set instance variable | see below |

The iteration hands each set variable's name as a typed symbol, and its value, to a closure that signals whether to continue or stop. Stopping ends the iteration before the remaining variables. It visits the variables and the values they held when the iteration began. A closure that assigns, removes, or adds the holder's instance variables changes the holder but never the visited set. The iteration dispatches no Ruby and surfaces no `Err`; for a holder that holds no instance variables, it visits nothing. A closure panic ends the iteration before the remaining variables and resurfaces on the Rust side, never crossing into mruby's frames.

##### Class variables and constants

A class or module handle's named class variables and constants support these operations:

| Operation | Behavior | Surfaces `Err` |
|---|---|---|
| class-variable read | through `TryConvert` into a requested type, walking the ancestry | the name resolves to no class variable, or the value does not convert |
| class-variable assign | any `IntoValue` value, in place | the receiver is frozen |
| class-variable presence | walking the ancestry | never; a total predicate |
| constant fetch | through `TryConvert` into a requested type | the name resolves to no constant, its `const_missing` hook raises, or the value does not convert |
| constant assign | any `IntoValue` value, in place; an unnamed class or module it binds is named by the constant's path when the receiver is `Object` or itself named | the receiver is frozen, or its `const_added` hook raises |
| constant presence | walking the ancestry | never; a total predicate |
| direct constant presence | on the receiver alone; true only for the receiver's own constant, never one inherited from an ancestor | never; a total predicate |
| constant removal | discards the former value; an absent constant is a no-op rather than an error | the receiver is frozen |

A top-level constant binds on the live `Mrb` handle as a constant assignment on `Object`, mirroring `magnus`'s `define_global_const`.

##### Global variables

A global variable belongs to the interpreter rather than to any value. So it reads, assigns, and removes on the live `Mrb` handle, symbol-or-name keyed:

| Operation | Behavior |
|---|---|
| read | an unset global reads as `nil` |
| remove | removing an unset global is a no-op |
| assign | reports no failure of its own, carrying only the one its key can surface |

Neither the read nor the removal dispatches Ruby or raises.

#### Classes, modules, and methods

##### Class definition

Class and module definition are methods on the live `Mrb` handle: `define_class(name, superclass)` and `define_module(name)` return typed `RClass` and `RModule` handles. Class definition is top-level on the `Mrb` handle and within a namespace through the `Module` trait. A name the namespace itself already binds (a top-level constant, for top-level definition) resolves as follows.

| Name already bound to | Result |
|---|---|
| an ordinary class (not a singleton class) whose superclass is the one given | that bound class itself, whatever modules are prepended to it; binding untouched |
| anything else, a class with a different superclass included | Rust `Err` carrying a `TypeError` |

##### Anonymous classes

The live `Mrb` handle also creates an anonymous class, given a superclass, and an anonymous module, mirroring `magnus`'s anonymous class and module creation. The result is an unnamed `RClass` or `RModule`, reachable only through the returned handle and never registered under a name in any namespace. It gains a name only when a constant assignment later binds it.

| Creation | Outcome |
|---|---|
| anonymous class | Rust `Err` when mruby rejects the superclass: a non-class, a singleton class, or `Class` itself |
| anonymous module | always succeeds |

##### Module trait operations

Methods register on those handles through the `Module` trait, and singleton methods on any instance-variable holder through the `Object` trait, mirroring `magnus::Module` and `magnus::Object`. Both accept Rust closures whose receiver, arguments, and return values cross the boundary through `IntoValue` / `TryConvert`. The `Module` trait also performs these operations.

| Operation | Effect |
|---|---|
| alias | aliases an existing method |
| include (Ruby's `Module#include`) | mixes another module in after the receiver in the ancestry; the receiver's own methods win |
| prepend (Ruby's `Module#prepend`) | mixes it in ahead of the receiver; the module's methods override the receiver's own |
| undefine (Ruby's `Module#undef_method`) | marks the name as not defined on the handle, even when an ancestor defines it |
| singleton undefine (`Object` trait) | undefines a singleton method |
| remove (Ruby's `Module#remove_method`) | deletes the method's own definition from the handle; the name reverts to any ancestor's method |

Removal strips the definition rather than masking ancestor lookups, which distinguishes it from undefinition. A definition, registration, alias, module inclusion or prepend, undefinition, or removal mruby rejects surfaces as a Rust `Err`. Rejections include a cyclic include or prepend and undefining a name absent from the handle and its ancestors. They also include removing a name not defined directly on the handle.

##### Name keys

Every operation keyed by a name accepts the name as a symbol-or-name key, mirroring `magnus`'s `IntoId`.

| Family | Name-keyed operations |
|---|---|
| definition | class, module, exception class, method, private method, module function, class method |
| lookup | class/module and built-in exception-class lookups on `Mrb`; class/module lookups within a namespace |
| variables | instance-variable, class-variable, constant, and global-variable operations |
| dispatch | method dispatch and the `respond_to?` test on a value |

Every key resolves to the `Id` it names: a string key by interning, an already-interned `Id` or `Symbol` key as the id it already is. A string key reaches the intern as a NUL-terminated C string, keyed on the bytes before its first NUL, or as a Rust string, keyed on all of its bytes. A consumer holding an `Id` or a `Symbol` reaches the operation without a redundant intern. The result is identical to passing the equivalent name, since both resolve to the same interned id. A method alias keys the new and the original name this way, each independently.

A key too long to intern names no symbol.

| Operation | Outcome for a too-long key |
|---|---|
| one that can report failure | surfaces the intern's `Err` as its own, without acting |
| total predicate | `false` |
| total read | `nil` |
| total removal | nothing done |

##### Exception classes

An exception class has a typed handle of its own, `ExceptionClass`, mirroring `magnus::ExceptionClass`. Building and raising an exception take this handle rather than the general class handle. A built-in exception-class lookup on `Mrb` yields one; it is the typed path to a built-in exception class (`RuntimeError`, `ArgumentError`, `TypeError`) for raising from registered code.

| Built-in lookup finds | Result |
|---|---|
| no constant under the name | Rust `Err` |
| a constant that is not a class | Rust `Err` |
| a class that is not an exception class | Rust `Err` |
| an exception class | `ExceptionClass` handle |

A consumer's own exception class is defined under a name from an exception-class superclass, yielding the handle directly, mirroring `magnus`'s `define_error`. Definition is top-level on the `Mrb` handle and within a namespace through the `Module` trait, symbol-or-name keyed. A name already bound resolves exactly as class definition resolves it: the bound ordinary class itself when its superclass is the one given, an `Err` otherwise. The handle registers methods and binds constants through the `Module` and `Object` traits, and yields the class handle for any operation that takes one.

##### Defined-name predicate

The class/module lookup family also answers, as a total boolean predicate, whether a class or module is defined under a given name. It is top-level on the `Mrb` handle and within a namespace through the `Module` trait, both symbol-or-name keyed. Unlike the fetching lookups, the predicate never raises.

| Name in that scope | Answer |
|---|---|
| bound | `true` |
| unbound, a name too long to intern included | `false`, rather than an `Err` |

It is the precondition test a consumer runs before a fetching lookup that would otherwise raise on a missing name.

##### Real class resolution

A class handle resolves to its real class: its singleton-class and include-class links are skipped, yielding the first user-facing class in the chain. A handle that is already a real class returns itself. The resolution walks the class structure and never raises. It is the named normalization a consumer reaches for after obtaining a handle that may not be a real class.

| Handle obtained through | May be |
|---|---|
| the singleton-class read or the class handle's downcast | a singleton class |
| the raw FFI seam | an include class |
| the value-level "class the value belongs to" | already the real class; needs no separate resolution |

The raw class of a value before that normalization may be a singleton or include class and demands VM-internal reasoning to use. It stays behind `beni::sys`.

##### Qualified path read

A class or module handle reads its fully-qualified path: the namespace chain leading to it. This is a total non-dispatching read that never raises. A consumer reaches for it to render a handle by its place in the namespace.

| Handle | Path read |
|---|---|
| class nested under modules `A` and `B` | `A::B::C` |
| top-level | the bare name |
| anonymous, with no place in any namespace | nothing |

The path is distinct from the handle's unqualified name read. The name read always answers a name, synthesizing one for an anonymous handle. The path read answers the qualified path or nothing, never a synthesized stand-in.

##### Receiver conversion

Every typed method registration, whatever its arity, hands the registered Rust function its receiver converted through `TryConvert`, mirroring `magnus`'s typed `self`. A function taking the receiver as a `Value` sees it unchanged. One taking a typed handle or a Rust value sees the receiver converted by that type's rule. A call failing more than one check raises the first failure in this order:

1. A fixed-arity registration's positional count.
2. The receiver's conversion.
3. Each argument's conversion.

A receiver that fails the conversion raises its exception to the Ruby caller before the body runs, as a failed argument does.

##### Positional arguments

A typed method registration declares a fixed count of required positionals and, after them, a count of optional positionals. Mirroring `magnus`'s trailing-`Option` arguments, the optional slots are the trailing parameters of the registered Rust function.

| Positional | Crosses as |
|---|---|
| required | its type, through `TryConvert` |
| optional, present in the call | `Some` of the argument converted through `TryConvert` |
| optional, omitted | `None` |

The registration derives the argument-spec aspec from the two counts.

| Declaration | Derived aspec |
|---|---|
| required only | the required aspec |
| required plus optional | the required-and-optional aspec, accepting the optionals while still requiring the leading ones |

A `TryConvert` failure on a supplied argument, required or optional, raises its exception to the Ruby caller before the body runs, as for the required-only form.

##### Block arguments

A typed method registration declares that it accepts a block. The block crosses as an `Option<Proc>` trailing parameter following the required positionals, mirroring `magnus`'s `Option<Proc>` block argument. The registration derives its aspec by adding the block flag, the aspec mruby uses to mark a method block-accepting, to the required aspec. The parameter is the typed `Proc` the consumer invokes through `Proc::call`; receiving a block needs no `beni::sys`.

A registered method asks whether it was called with a block through a total predicate on the `Mrb` handle, mirroring magnus's `Ruby::block_given_p`. It reads the current call and never raises. It is a plain boolean question: it surfaces neither the call frame's block slot nor any other VM-internal structure.

| Call | Block parameter | Block-given predicate |
|---|---|---|
| passes a block | `Some` | `true` |
| passes none (mruby leaves the block slot nil) | `None` | `false` |

##### Method returns

A registered method's body returns one of a closed set, mirroring `magnus`'s
sealed `ReturnValue`:

| Body returns | The Ruby caller receives |
|---|---|
| an `IntoValue` value | the converted value |
| a `Result` of an `IntoValue` value | the converted `Ok` value; an `Err` raised |
| a fiber yield, or a `Result` of one | the value the fiber's next resume passes, the fiber suspended until then; an `Err` raised |

The set is sealed: a consumer adds no return kind and converts no return
outside the registration, so a fiber yield takes effect only as the method's
return.

##### Call frame reads

A method registered for any arity receives the call's arguments as a slice, mirroring `magnus`'s `method!(f, -1)`. The slice is the method's own copy, valid whatever the body re-enters.

| Call passes | Slice holds |
|---|---|
| positionals | each, in order |
| a non-empty keyword hash | the call's own hash, as one trailing value |
| no arguments | nothing |

Handing the slice over leaves the frame unchanged, so a scan read still finds the call's keywords. The body reads any further shape from its frame.

| Read | Returns | Failure |
|---|---|---|
| scan read | the frame projected into typed parts | `Result` |
| single-argument read | the one required argument | `Result` |

A call not fitting a read's shape surfaces as an `Err` carrying the exception raised for the mismatch. Mismatches are too few or too many positionals, a wrong argument type, or a missing required block. Nothing raises past the body, which decides how the failure leaves it. The single-argument read's shape is exactly one positional; the keyword hash stands in for it when the call passed keywords and no positional.

##### Scan read

The scan read mirrors `magnus`'s `scan_args`, reading the frame where magnus reads an argument slice. It composes its shape from six parts, handed back separately so a body reads any argument shape mruby accepts in one read. Each part is declared by the type it hands back, and is absent when declared as `()`.

| Part | Binds |
|---|---|
| required positionals | each through `TryConvert` |
| optional positionals | `Some` of the converted value when supplied, `None` when omitted |
| splat | the remaining positionals: an array handle, or values each through `TryConvert` |
| trailing required positionals | after the splat, each through `TryConvert` |
| keyword bucket | the call's keyword arguments, always as a hash |
| block | a required block (absent: `ArgumentError`) or an optional one (absent: `None`) |

An array-handle splat stays valid for the whole call, whatever the body re-enters. A scan read without a block part ignores a block the call passes. A scan read whose only parts are an array-handle splat and an optional block fits every call and always answers `Ok`.

##### Scan read keywords

The keyword bucket holds the call's keyword arguments apart from the positionals. Keywords land as follows.

| Case | Outcome |
|---|---|
| call passed no keywords | the bucket is an empty hash, not absent |
| explicit positional hash the caller wrote | never captured; stays among the positionals |
| non-empty keyword hash, scan read without a keyword part | read as its last positional |

Every later read in the same call sees that last positional there.

##### Keyword read

The named keyword read mirrors `magnus`'s `get_kwargs`. It takes a keyword hash and two lists of symbol-or-name keys: the required keywords and the optional ones. It hands back the required values, the optional values, and a rest, each part declared by its type as the scan read's are. Each value crosses through `TryConvert`, and the given hash is left unchanged.

| Case | Outcome |
|---|---|
| optional keyword the hash lacks | binds `None` |
| required keyword the hash lacks | `ArgumentError` |
| keyword neither list names, rest declared | held in the rest, a new hash |
| keyword neither list names, rest absent | `ArgumentError` |
| key list length differs from its part's declared count | programming error; the read panics |

##### Instance construction

Constructing an instance of a class handle runs Ruby's `Class.new`: it allocates the object and runs its `initialize` with an argument slice. A raising `initialize` surfaces as a Rust `Err`. This mirrors `magnus`'s `Class::new_instance`.

A module function registers on a module handle in one call and becomes two methods, the way `Math.sqrt` is callable both ways.

| Becomes | Reached as |
|---|---|
| a private instance method | a bare helper inside a class that mixes the module in |
| a singleton method on the module object | `Math.sqrt` |

Class methods need no separate form: a singleton method defined on a class is its class method, mirroring magnus.

##### Typed data contract

A Rust-owned value backs an mruby object through the data-carrier mechanism (`CDATA`), in `magnus`'s typed-data shape. A Rust type opts in by implementing the `unsafe` `TypedData` trait.

| Trait item | Meaning |
|---|---|
| data type | a `'static` descriptor carrying the name mruby diagnostics show; its release drops the payload |
| class | the class its values wrap as, optionally per value: that class or a subclass |
| `mark_carriers` | marks, for one interpreter, the class the implementation's own `class` answers |

The implementer upholds the trait's contract: every class it names is marked to carry data, as below, before a value wraps into it. The macros below replace `mark_carriers` with one preparing every class they name. A data type belongs to one Rust type, so a carrier of `T`'s data type holds a `T`.

##### Carrier marking

A class is marked so its instances are data carriers holding Rust data, and a class defined from a marked superclass is marked too. Marking is fallible.

| Class | Mark |
|---|---|
| instances are plain objects or data carriers | accepted |
| a singleton class, whose one instance is the object it belongs to | `Err` carrying a `TypeError` |
| instances have a built-in layout of their own | `Err` carrying a `TypeError` |

Built-in layouts are an exception, a string, an array, a hash, a range, a proc, a number, and a class or module, subclasses included. A rejecting class stays unmarked, so every instance keeps the layout mruby's own methods read.

##### Allocator undefinition

A class's default allocator is undefined in one call, mirroring `magnus`'s `undef_default_alloc_func`. The effect covers the class and any class later defined from it. A singleton class, which Ruby never allocates through, is left unchanged.

| Afterwards | Result |
|---|---|
| Ruby's `new` and `allocate` | mruby's `TypeError` "allocator undefined for *class*" |
| a wrap into it | still allocates |
| `dup` / `clone` of one of its carriers | still allocates |

##### Typed data wrapping

A `TypedData` value wraps as a new instance of the class its type names for it, or of a given class. A given class is the type's class or a subclass of it, which a debug build asserts. The result is an untyped `RTypedData` handle or a typed `Obj<T>` handle, mirroring `magnus`'s `wrap` / `wrap_as` and `obj_wrap` / `obj_wrap_as`.

| Event | Behavior |
|---|---|
| wrap, type keeping its contract | does not fail |
| wrap into a class that cannot carry data | contract broken: reclaims the payload and panics rather than raising across the boundary |
| carrier collected | the collector releases the payload, on whichever thread reaches the interpreter |

The mruby garbage collector owns a wrapped payload and releases it on whichever thread reaches the interpreter, so a type is `TypedData` only if it can cross threads. The collector never traces into a payload. A value a payload keeps past the frame that stored it stays valid only through a GC validity rule exemption. The exemptions are a hidden instance variable of its carrier, or a root.

##### Payload access

A wrapped payload reads back as `&T` through three paths.

| Path | Answers |
|---|---|
| the `TryConvert` rule for `&T` (a method's receiver or argument) | `&T` |
| an `RTypedData` handle's read | that rule's `Result` |
| an `Obj<T>` handle | dereferences to the payload it was converted or wrapped with |

A payload is installed only into a carrier holding none, and nothing on the typed surface replaces or removes one once a carrier holds it. A reference therefore stays valid for as long as its carrier stays reachable.

##### Payload installation

mruby gives a class whose instances are data carriers no allocator of its own: its `new` and `allocate` while its default allocator stands, and mruby's `dup` and `clone` of one of its carriers, make a carrier holding no payload. A payload is installed into such a carrier through its `RTypedData` handle, so a Ruby-side `initialize` or `initialize_copy` gives the carrier its payload. `magnus` has no counterpart, CRuby's allocator filling a payload as it allocates.

| Carrier | Install |
|---|---|
| holds no payload, frozen or not | installs the payload; the carrier converts as `T` from then on |
| already holds a payload | refused: the offered payload handed back, the held one unchanged |

The carrier's class is `T`'s class or a subclass of it, which a debug build asserts. An install into a carrier of another class completes, and the carrier converts as `T`.

##### Carrier copies

mruby's `dup` and `clone` of a data carrier copy the object without its payload, leaving a carrier that holds none and converts to no `T`. A type that is `TypedData` and `Clone` copies its payload through `typed_data::Dup`, mirroring `magnus`'s.

| Method | Behavior |
|---|---|
| `dup` | answers a clone of the receiver's payload |
| `clone` | copies the receiver as mruby's `clone` does; answers the copy as `Obj<T>` |

A method returning `dup`'s answer wraps it as a new instance. `clone` installs a clone of the payload into the copy, unless the copy's `initialize_copy` installed one of its own, which the copy keeps. It keeps the receiver's singleton class and frozen state and runs its `initialize_copy`. It takes no arguments, as mruby's own does not: an argument surfaces the `ArgumentError` mruby raises for a wrong argument count. A raising `initialize_copy` surfaces as an `Err`.

##### Typed data macros

`#[beni::wrap(class = "…")]` on a struct or enum implements `TypedData` for it, as does `#[derive(beni::TypedData)]` with a `#[beni(class = "…")]` attribute. They mirror `magnus`'s `wrap` and `TypedData` derive and generate the same implementation. The derive takes no companion derive, `beni` carrying no `DataTypeFunctions`.

| Attribute | On | Meaning |
|---|---|---|
| `class` (required) | the type | the constant path its values wrap as |
| `name` | the type | the data type's name; defaults to the `class` path |
| `class` | an enum variant | that variant wraps as this class: the type's class or a subclass |

Every other enum variant wraps as the type's class. Each attribute is a string holding no NUL byte. Every other attribute is a compile error, `magnus`'s `mark`, `size`, `compact`, `free_immediately`, `wb_protected`, `frozen_shareable`, `unsafe_generics`, and `opaque_attr_reader` included. A type with generic parameters or lifetimes is a compile error too.

##### Class path resolution

A `class` path's segments are fetched one after another, so `"Outer::Inner"` names a nested class.

| Segment | Fetched as |
|---|---|
| the first | a constant of `Object` |
| each later one | a constant of the segment before it |
| one nothing is bound under | a `const_missing` hook stands in |

A segment is fetched as the typed surface fetches any constant. No `const_get` method takes part, so a program defining one changes nothing a path resolves to.

##### Carrier preparation

`TypedData::mark_carriers` prepares every class a generated implementation names, the type's own and each enum variant's, in the interpreter at hand. This is how the macros uphold the `TypedData` contract. For each class it does the following:

1. Resolve the path.
2. Mark the class to carry data.
3. Undefine the class's default allocator.
4. Hold the class in the interpreter's carrier record under that path.

It surfaces an `Err` when a path resolves to no class, resolves to a value that is not a class, or names a class that refuses the mark. Marking a path the record already holds resolves it again and replaces what it holds. The embedder calls `mark_carriers` for each type in each interpreter while installing its gems, before any Ruby program runs, so every path resolves against the classes the embedder defined.

mruby hands a class its superclass's mark and allocator state when the class is defined.

| Class defined from the type's class | Carries |
|---|---|
| before `mark_carriers` marks it | neither; wrapping into it breaks the contract as an unmarked class does |
| by a Ruby program | both, since marking while gems install precedes every class a Ruby program defines |

##### Carrier record use

A carrier record is kept inside the interpreter holding it and keeps its class reachable for as long as that interpreter lives. It is named as no Ruby global variable, so no guest program reads or writes it. No class crosses from one interpreter to another, and each interpreter is marked on its own.

| Reader | Answers |
|---|---|
| a generated `TypedData::class` | the class the carrier record holds for the path |
| a generated enum variant's class | the class the carrier record holds for the path |
| either, for a path the record does not hold | panics, naming the `mark_carriers` call that puts it there |

Wrapping resolves no constant and dispatches no Ruby method, so what a Ruby program binds over a path changes no class a value wraps into.

##### Inline struct contract

A Rust value also backs an mruby object as an inline struct (`ISTRUCT`), mruby's layout for plain data stored inside the object itself. A Rust type opts in by implementing the `unsafe` `InlineStruct` trait, which requires `bytemuck::Pod` and `Send`. The type names the class its values wrap as and the name diagnostics show for it. `InlineStruct::mark_carriers` prepares that class in one interpreter.

| Constraint | Consequence |
|---|---|
| size exceeds three pointer widths | does not compile as an `InlineStruct` |
| alignment exceeds a pointer's | does not compile as an `InlineStruct` |
| `Pod` admits no `Value` or typed handle | holds no value; the collector never traces its payload; no instance variables |

##### Inline struct marking

`mark_carriers` resolves the class path as the `TypedData` macros resolve theirs, marks the class so its instances are inline structs, and undefines its default allocator. It holds the class in the interpreter's carrier record together with the type it belongs to, and as that type's class, replacing the class an earlier `mark_carriers` of the type held. A class belongs to an `InlineStruct` type when the nearest class in its ancestry the record holds, the class itself or a superclass, is held for that type.

A generated `InlineStruct::class` answers the type's class the record holds, and panics naming `mark_carriers` when the record holds none.

| Class | Outcome |
|---|---|
| instances are plain objects | accepts the mark |
| belongs to the same type | accepts the mark |
| any other, an inline-struct class mruby or a C gem defined included | refuses with a `TypeError`; stays unmarked |
| path resolves to no class, or to a non-class value | `Err` |

##### Inline struct handle

An inline struct converts back only to the type its class belongs to. A value converts to `T` when it is an inline struct whose class belongs to `T`. Any other value surfaces the `TypeError` "wrong argument type *value* (expected *T name*)", *value* named as mruby's own type check names it.

`Inline<T>` is the typed handle over an inline struct of `T`, mirroring `Obj<T>`. Nothing hands out a reference into the payload.

| Item | Behavior |
|---|---|
| `Inline::new` | wraps a value as a new instance of the type's class |
| `get` | answers a copy of the payload |
| `set` | replaces the whole payload |
| `set` on a frozen receiver | surfaces mruby's `FrozenError`; payload unchanged |

Wrapping into a class whose instances are not inline structs breaks the trait's contract, and the wrap panics rather than raising across the boundary. Wrapping into an inline-struct class of another type breaks it too: the wrap completes, and the value converts as that type, its payload read as plain bytes. mruby's `dup` and `clone` of an inline struct copy its payload, so a copy converts as the original does. Ruby's `new` and `allocate` raise as for any class whose allocator is undefined, so a type defines the constructor its Ruby callers use.

##### Inline struct macros

`#[derive(beni::InlineStruct)]` with a `#[beni(class = "…")]` attribute, and `#[beni::wrap(class = "…", inline)]`, implement `InlineStruct` for a struct. They take `class` and `name` as the `TypedData` macros take them. A type they implement it for also converts by value.

| Conversion | Behavior |
|---|---|
| `TryConvert` | answers a copy of the payload |
| `IntoValue` | wraps the value as `Inline::new` does |

They reject at compile time an enum, a union, a type with generic parameters or lifetimes, and every attribute but `class`, `name`, and `inline`. `inline` is accepted by `wrap` alone.

#### Garbage collection

Arenas and roots govern which values stay reachable. Collection timing governs when the collector reclaims the unreachable rest.

| Concern | Typed surface |
|---|---|
| Arena scope | `Mrb::arena_scope` |
| Root | `Mrb::gc_register_forever`, `Mrb::gc_root` |
| Collection timing | `Mrb::full_gc`, `Mrb::incremental_gc` |
| Heap region | `Mrb::gc_add_region` |

##### Arena scopes

`Mrb::arena_scope` bounds GC arena growth across a region of Rust code. Values that cross out to Rust inside the scope stay reachable until the scope ends. The scope's end releases the arena protection taken inside it.

| Scope ends by | Survivor |
|---|---|
| `keep` | the one value it names, re-protected |
| dropping the scope | none |

Arena protection reaches only as far as the C frame that opened the scope. A value a Rust caller holds past that frame needs a root.

##### Handed-out values

Every value the typed surface hands to a Rust caller stays reachable from the moment it crosses out. It stays reachable until the innermost arena scope then open ends. It stays reachable at most until the C frame it crossed out in returns to mruby. This holds even after the object that held it lets it go.

| Value handed out | Origin |
|---|---|
| created | by the typed surface |
| read out | of an array, a hash, or a variable |
| argument | of a method |
| exception | however it was produced |

A caller therefore renders an exception's message and backtrace without rooting it first. A value held past that point needs a root.

##### Roots

A **root** keeps a value reachable independently of the arena and of any Ruby reference to it. The two rooting shapes differ in whether the root is ever released.

| Shape | Answers | Root released |
|---|---|---|
| `Mrb::gc_register_forever` | nothing | never |
| `Mrb::gc_root` | `Result` of a `GcRoot` guard | when that guard drops |

`Mrb::gc_register_forever` roots a value for the interpreter's remaining lifetime, so the value is never reclaimed. It is the shape for a value an embedder holds as long as the VM itself, such as a cached class handle. Rooting an immediate value is a no-op, immediates being values the collector never reclaims.

##### Releasable roots

`Mrb::gc_root` is the shape for a value held across a round trip out of the VM and released afterwards.

| Event | Effect |
|---|---|
| take fails | `Err`; no root taken |
| drop one `GcRoot` | releases that root alone |
| last `GcRoot` over the value drops | value no longer rooted |
| mruby refuses a release | value rooted for the interpreter's remaining lifetime |

Each guard owns one root, so roots over the same value are independent and no drop affects another. The value stays rooted while any `GcRoot` over it lives. Reachability from the arena or from Ruby is a separate matter, and neither depends on a root. Taking a root grows the record of roots, so it is fallible. Releasing one cannot fail into an unrooted value. The failure a consumer can meet is over-retention, never a value collected while still held.

##### Root records

Rooting keeps its record inside the interpreter. A guest program that enumerates globals sees one entry per rooting shape in use.

| Rooting shape | Entry owner |
|---|---|
| never-released | mruby's own |
| releasable | beni's |

Neither entry is named as a Ruby global variable, so no guest program can read or write the record; it is visible to enumeration alone.

A consumer reaching mruby's own root registry through `beni::sys` owns an invariant the typed shapes encode. That registry is keyed by value rather than by registration. Removing a value removes every root over it, and a released root cannot be told from another holder's. `GcRoot` supplies the per-root identity that makes independent release well defined.

##### Collection timing

`Mrb::full_gc` and `Mrb::incremental_gc` drive collection directly.

| Method | Effect |
|---|---|
| `full_gc` | one complete collection cycle |
| `incremental_gc` | advances the collector a single step |

Both are total: they return nothing, never raise, and are safe to call whenever the VM is alive. A disabled or mid-collection collector ignores the request. Both graduate because correct use needs no reasoning about VM internals.

##### Heap regions

`Mrb::gc_add_region` hands the collector a caller-owned byte buffer to carve into heap pages. Objects can then live in memory the caller placed, not only in pages the allocator hands out.

| Buffer | Answer |
|---|---|
| holds one page or more | number of heap pages yielded |
| too small to hold one | zero |

It adds to the collector's pages without capping them; once they are exhausted, the collector grows through the allocator as it otherwise would. The call takes the buffer by move for the process's whole lifetime. The caller cannot reach it again, and the same buffer cannot be handed over twice. mruby never frees it — the memory outlives the interpreter. Alignment within the buffer is the collector's concern, not the caller's.

#### Gems and blocks

The `Gem` trait is the unit of Ruby surface a Rust crate ships.

```rust
trait Gem {
    fn init(mrb: &Mrb) -> Result<(), Error>;
}
```

The embedder invokes each gem's `init` with the live interpreter handle during interpreter setup. The gem defines its classes, modules, and methods there. An `Err` from `init` aborts setup and surfaces to the embedder.

##### Block calls

A typed `Proc` handle wraps an mruby block. `Proc::call` invokes it with an argument slice under exception protection. A non-local exit surfaces as a Rust `Err` instead of unwinding across FFI.

| Block exits by | `Proc::call` answers |
|---|---|
| normal return | `Ok` with the returned value |
| a raised exception | `Err` |
| a thrown `break` / `return` object | `Err` |

`ReprValue::as_break` views an escaped value as a typed `Break` when it carries mruby's break tag, and yields no view for any other tag. `Break` exposes the value the break carries.

Whether a break is a real `break`, a `return` aimed past a frame, or a plain raise is the consumer's classification. The call-info frame indices that distinguish those cases are mruby VM internals with no stable public accessor. The typed surface does not expose them, so a consumer that must classify reaches them through the `beni::sys` escape hatch.

##### Rust-defined procs

The `Mrb` handle builds a `Proc` whose body is Rust, mirroring `magnus`'s `Ruby::proc_new` and `Ruby::proc_from_fn`. Building one never fails.

| Operation | Body |
|---|---|
| proc from a function | a plain function |
| proc from a closure | a closure that may be called more than once and can cross threads |

The body receives the interpreter handle, the call's arguments as a slice laid out as a method registered for any arity receives them, and the call's block as an `Option<Proc>`. It returns one of a closed set, mirroring `magnus`'s sealed `BlockReturn`:

| Body returns | The proc's caller receives |
|---|---|
| an `IntoValue` value | the converted value |
| a `Result` of an `IntoValue` value | the converted `Ok` value; an `Err` raised |

A raise from the body reaches the proc's caller: a Ruby caller sees the exception, and `Proc::call` answers it as an `Err`. A closure called again while it is already running, through its proc or a copy of it, raises `RuntimeError` to that second caller, and the running call is unaffected.

A proc and every copy made of it share one closure. The interpreter drops the closure once it has reclaimed all of them, or when it closes. The collector never traces into a closure, so a value the closure captures stays valid only through a root.

A Rust-defined proc is backed by a C function: dumping it and creating a fiber from it each answer `Err`.

##### Proc dumps

A Proc answers its compiled form as a byte buffer of its own — the bytecode a bytecode load reads back.

| Proc or dump | Answer |
|---|---|
| backed by a C function | `Err` carrying an exception |
| dump mruby cannot complete | `Err` carrying an exception |

Both failures surface as every other failure the typed surface reports does. A dump carries the instructions alone. The line numbers a loaded program's exceptions are backtraced from, and the local variable names, are each carried only when the caller asks for that one.

#### Fibers

A `Fiber` handle runs a Ruby-defined block as a fiber, mirroring `magnus`'s
`Fiber`. Its operations are carried by the `fiber` capability feature.

| Operation | Behavior |
|---|---|
| create | a fiber from a `Proc` handle, not yet started |
| resume | starts the fiber with an argument slice as its block's arguments, or continues a suspended one with the slice as the value its pending yield answers; answers the value the fiber next yields, or its block's value once the block finishes |
| alive test | whether the fiber can still be resumed — false once its block has finished |
| fiber yield | built from an argument slice on the `Mrb` handle and returned from a registered method running inside a resumed fiber; the return suspends the fiber and hands the slice to its resumer |
| current | on the `Mrb` handle, the fiber `Fiber.current` answers when dispatched — the running fiber, or the root fiber outside any resumed one |

A slice handed across a switch arrives as one value:

| Slice holds | Arrives as |
|---|---|
| nothing | `nil` |
| one value | that value |
| several values | an Array of them |

A fiber yield answers Rust no resumed value: the value the next resume passes
reaches the method's Ruby caller. Transferring to a fiber and raising into a
fiber are not carried.

#### User data

An interpreter holds at most one piece of user data — a Rust value of any type that can cross threads — in the auxiliary-data slot mruby's state carries for an embedder. The interpreter drops the value it holds when it closes.

| Operation | Who | Outcome |
|---|---|---|
| read | any borrow of the interpreter | borrows the held value in place |
| install | only the handle's owner | refused when the slot holds a value |
| take | only the handle's owner | yields the held value; slot left empty |

Readers include a gem's installation and a registered method. A read lasts as long as the borrow of the interpreter it was read through. A refused install hands the offered value back and leaves the held one in place, so replacing a value is taking it first.

A read or a take names the type it expects. Naming a type other than the one held, or reaching an empty slot, answers nothing. A take that answers nothing leaves the slot as it was.

mruby publishes no call for the slot, so it belongs to this surface: a write to it through `beni::sys` is the writer's own unsafe act.

#### Loading precompiled bytecode

No part of this section is carried by the `compiler` capability feature.

Loading a precompiled bytecode blob runs the program at the interpreter's top level. The blob is a byte slice carrying its own length, and it is the form a Proc's dump answers.

| Outcome | Answer |
|---|---|
| program runs | `Ok` with its result value |
| blob unreadable as a program | `Err` carrying a `ScriptError`; nothing runs |
| program raises while it runs | `Err` carrying that exception |

The `ScriptError` message distinguishes four conditions:

| Condition |
|---|
| blob shorter than the format's header |
| header ident not the format's |
| format version the interpreter does not read |
| body fails validation |

When the loaded program raises, the pending exception is cleared from the handle as it crosses out — the contract a load under a compile context answers. Neither failure carries an outcome of its own: both are the `Err` every fallible operation on this surface answers with.

#### Compiling and running source

The `compiler` capability feature carries everything in this section. Both operations take a slice of Ruby source under a compile context.

| Operation | Success |
|---|---|
| compile and run | `Ok` with the program's result value |
| compile without running | the compiled program as a typed `Proc` |

##### Compile contexts

A compile context is created against a live interpreter with a filename and released when it is dropped. It never outlives that interpreter and stays on the thread that made it.

| Filename length | Creating the context | Every load and compile under it |
|---|---|---|
| under `UINT16_MAX` bytes | accepted | stamps the filename |
| `UINT16_MAX` bytes or more | accepted | fails with the creating interns' `ArgumentError` |

The filename is stamped onto everything compiled through it, so the exceptions the compiled program raises carry a source-line backtrace. The stamp is a symbol each load interns as it compiles, which is why creation accepts any filename. A filename of `UINT16_MAX` bytes or more is too long to be a symbol. One context serves any number of loads and carries the top-level local variables across them, so successive loads see each other's locals.

##### Loads and compiles

The source is a byte slice carrying its own length, so it needs no terminating NUL; the bytes need not be valid UTF-8. Failures surface as a Rust `Err`, distinguished by what the error carries rather than by the text of a message.

| Failure | `Err` carries |
|---|---|
| source does not parse | a parse message |
| filename too long to be a symbol | the exception |
| codegen failure | the exception |
| raise while the program runs | the exception |

Every failure that carries the exception clears the pending exception from the handle as it crosses out. Only the exception carries a backtrace. The two operations differ in what they produce and in nothing else, the warnings the context answers included. Whether an operation stops before running is settled per operation and is never a state the context keeps. No load's meaning depends on what an earlier call left behind.

##### Compiled programs

A compiled program is invoked through `Proc::call` like any other block, and invoking it runs the program at the interpreter's top level.

| Program | Context's top-level local variables |
|---|---|
| run by the context itself | reach it |
| invoked by the caller | start without them |

It is a value like any other, so outliving the arena scope that produced it needs a root.

##### Parse messages

A parse message carries one compiler diagnostic's line, column, and message text, read through accessors rather than exposed as fields.

| Compiler recorded | Parse message for unparsable source |
|---|---|
| one or more diagnostics | the first one recorded |
| none | zero line, zero column, empty text |

The no-diagnostic case never reads a diagnostic that was never written.

##### Warnings

A context answers the warnings the compiler produced for its most recent load as parse messages. Warnings do not change a load's outcome: a load that produces warnings and no error still yields its result value as `Ok`.

| Context state | Warnings answered |
|---|---|
| no load run | none |
| most recent load produced none | none |
| most recent load produced some | those, as parse messages |

The compiler's diagnostics never reach the process's standard error. The returned parse message and the context's warnings are the only place they surface. Where a diagnostic is written is the host's choice, not beni's.

##### Contextless loads

`Mrb` compiles and runs a slice of Ruby source without being given a compile context. It borrows an unnamed one for the load and releases it when the load returns. A failure surfaces exactly as under a caller's context: a parse failure as a parse message, a codegen failure or a raise as the exception.

| Context | Filename stamp | Captured warnings |
|---|---|---|
| caller's own | stamped; source-line backtrace | answered by the context |
| borrowed | none; no source-line backtrace | gone when the load returns |

The borrowed context differs only where a context is the thing that would carry the difference. A caller who wants either holds a context of their own.

##### Backtraces

An error answers its backtrace as a list of rendered frames.

| Error | Backtrace |
|---|---|
| carries no exception | empty list |
| exception holds no backtrace | empty list |

#### Graduation, safety, and coverage

##### GC Validity Rule

The safe API cannot cause undefined behavior while the GC validity rule
holds. A value that crossed out to Rust — created or read — is not used after
either end below. A survivor carried out through `keep` counts as crossing
out where its scope was opened.

| End | Reached when |
|---|---|
| Arena scope | the innermost arena scope open as it crossed out ends |
| C frame | the C frame it crossed out in returns to mruby |

A value is exempt while something keeps it reachable for the collector, which
lets it outlive the frame that made it. The type system does not enforce the
rule; the consumer upholds it.

| Kept reachable by | Exempt for as long as |
|---|---|
| a root | the root lives |
| a hidden instance variable | the holding object stays reachable and the variable keeps that value |

##### Domain Crossing

The typed and raw domains meet asymmetrically. This is `magnus`'s `rb_sys`
asymmetry, and both directions sit beside the raw bindings as they do there.

| Direction | Form | Safety |
|---|---|---|
| typed → raw | a value handle answers its value, an `Id` its interned id | safe |
| raw → typed | a raw value or id crosses in | `unsafe` |

What the caller does with a raw form it read is a raw call, already `unsafe`
on its own account. Nothing about a raw value or id says it came from the
interpreter it will be used against. The typed surface trusts what it is
handed rather than re-testing it, so a wrongly crossed one reaches operations
that read it as the thing it claims to be.

A class handle is a value handle like the others, as a class is in `magnus`.
Its raw form is the value; the class pointer is what a raw binding unboxes
from that value. A raw class pointer reaches the typed domain only boxed as a
value and through the checked downcast.

##### Thread Contract

An interpreter crosses threads; it is never reached from two at once. It is
carried rather than shared.

| Use | Typed surface |
|---|---|
| one thread hands an interpreter to another | permits the move |
| separate threads each hold their own | permits |
| two threads reach one interpreter | refuses the share; the consumer supplies its own mutual exclusion |

A guard that borrows the interpreter — an arena scope, a root, a compile
context — pins both to the thread they were made on for as long as the guard
lives. A typed handle or `Id` crosses as freely as the interpreter does and
means something only against the interpreter that produced it. The type system
does not enforce that pairing; the consumer upholds it, as with the GC
validity rule above.

##### Graduation Bar

A capability reaches the safe typed surface only when the wrapper can encode
its invariant — a lifetime, a carrier type, or a runtime check — so a caller
uses it without reasoning about mruby's VM internals. This bar is stronger
than freedom from undefined behavior.

| Case | Form |
|---|---|
| invariant encodable and `magnus` gives no `unsafe` form | safe typed surface |
| `magnus` gives an `unsafe` form | `unsafe`, encodable or not |
| not encodable; a typed shape still carries the value | typed `unsafe fn` on the `beni` surface |
| not encodable; VM-internal value, no typed shape to add | raw `beni::sys` binding |

The safe surface's shape is `magnus`'s: one a `magnus` consumer reaches for
through `unsafe` is not one this surface makes safe under the same name. A
typed `unsafe fn` leaves one caller-owned invariant unencoded. VM-internal
values include call-info frame indices and VM-object internals, where a
safe-looking wrapper would misrepresent its sharpness. A capability unsafe
only for want of an unbuilt carrier graduates once the carrier exists, unlike
one permanently VM-internal. Closing a consumer's `beni::sys` use to zero is
not a goal; any unexposed C API stays reachable there.

##### Internal Symbol Admission

mruby stages its whole include tree beside an archive, its library-internal
header included, so what the crate can reach is wider than what mruby
publishes for an embedder. The typed surface follows what is published and
reaches past it only under this admission rule.

| Aspect | An internal symbol is admitted |
|---|---|
| Granularity | one at a time, never by taking its header in whole |
| Need | only where no published symbol delivers the capability |
| Safety | only where the graduation rule above can encode its invariants |
| Carrier | for the typed item that carries it, never on its own |
| Record | with what settles it |

An admitted symbol lands on the safe typed surface rather than beside it.
Nothing enters on the chance that a consumer might one day want it.

##### Coverage Measure

`docs/api_coverage.md` measures how far the typed surface has graduated
mruby's embedder API — the functions and macros an embedder calls across the
public embedder headers. A capability the typed surface graduates through a
Rust-native construct rather than the matching C symbol counts as covered
through that construct.

| Rust-native construct | Covers |
|---|---|
| a typed handle's downcast read from the value tag | the per-type `_p` macro it stands in for |
| a typed method definition's required and optional arity counts and block-accepting flag | the argument-spec aspec it declares |

The derived aspecs are the required, the required-and-optional, the
any-arguments, and the block aspecs.

`mrb_get_args` is one symbol whose format string is a vocabulary of argument
specifiers, so that vocabulary is measured as its own lens. Every specifier
is covered through the typed surface, and the lens records which surface
covers each one.

| Surface covering a specifier |
|---|
| a part of the scan read |
| a read composed with a conversion |
| the named keyword read |
| the typed method registration that declares it |

##### Shared Capability

Symbols carrying the same capability are covered together.

| C symbols | Coverage |
|---|---|
| one defined as another | covering either covers both |
| two differing only in what a Rust caller already expresses otherwise | the graduated item covers both |
| one a graduated item cannot express | not covered by it, however close their purposes |

What a Rust caller already expresses otherwise is a length the byte slice
carries or an argument count the slice carries. Such a pair is recorded with
what the Rust shape carries in the C form's place.

##### Measure Boundary

API an embedder cannot call never enters the measure: what the headers publish
for an embedder is the whole of it.

| API | In the measure |
|---|---|
| a library-internal header's declarations | never, however plainly staged |
| compile-time, debug assertion, and internal helper macros | never |
| an internal symbol the admission rule above lets through | no; recorded apart from the ratio |
| a symbol a capability feature carries | covered; the measure names the feature |

An admitted internal symbol is neither embedder API nor API still owed.
Turning a capability feature off subtracts nothing from the measure.

##### Measure Exits

What does enter the measure leaves again only for a reason the measure
records.

| API | Leaves as |
|---|---|
| public API the typed surface deliberately does not carry | declined |
| a capability awaiting a carrier | does not leave; stays as the work it is |
| API a build's ABI lacks because a compile-time flag gates it | flag-gated |

Declined API is what the graduation rule above leaves in `beni::sys` for want
of a typed shape to add, named for the measure. A capability awaiting a
carrier is not declined. Flag-gated stays distinct from declined because
letting a consumer choose its ABI turns the flag-gated set into work while the
declined set stays declined.

Each reason names what settles it — a statement in this specification, the
graduation rule above, or the vendored source it reads from — so a
classification can be reviewed rather than taken on trust. What remains is the
embedder API a build's ABI intends to carry, so a fully graduated surface
measures complete.

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
| A selected toolchain whose built-in pair carries no checksum for the build platform | `beni:vendor:setup` aborts and names the toolchain and the build platform, nothing downloaded |
| `build_config` naming a path that does not exist | `beni:build` aborts and names the missing config path, no archive built |
| `beni:build` with a `target` declaration naming a target the build config does not define | verification fails, each missing archive reported |
| A build config selecting the `wasi` toolchain with no wasi toolchain file staged | `beni:build` aborts, mruby naming the unknown toolchain |
| `beni:config` with no `build_config` declaration | task fails, nothing generated |
| `beni:config` with the configured `version`'s mruby source not staged | task fails and names the missing source, nothing generated |
| `beni:config` targeting an existing file | generation refuses, existing config untouched |
| A staged path with no compile-flags sidecar | `beni-sys` build fails and names the compile-flags sidecar |
| A compile-flags sidecar the crate cannot read — no line naming the archive, the compiler, the flags, or the libraries, or a token in a form it cannot attribute to a toolchain | `beni-sys` build fails and names the sidecar, never reading a partial set from it |
| No archive discovery variable set, outside a documentation build | `beni-sys` build fails and names the variables it consults |
| A documentation build whose documentation bindings are absent | `beni-sys` build fails and names the bindings it expected |
| `MRUBY_LIB_DIR` or `BENI_VENDOR_DIR` set but the archive its sidecar names is absent | `beni-sys` build fails and names the expected path |
| Discovered archive below the supported mruby floor | `beni-sys` build fails and names the archive's version |
| Discovered archive whose headers state no mruby version | `beni-sys` build fails and names the headers it read |
| Cross-compiled build for a cargo target that is neither wasm32 nor the other macOS architecture | `beni-sys` build fails and names the unsupported target |
| Cross-compiled build without `MRUBY_LIB_DIR` | `beni-sys` build fails |
| A cross-build whose `MRUBY_LIB_DIR` names an archive built for the other macOS architecture | discovery resolves, the link fails; the crate reads the archive's sidecar, never its architecture |
| wasm32 build missing its archive or the wasi-sdk toolchain | `beni-sys` build fails |
| The wasi-sdk root in effect (`WASI_SDK_PATH` when set, `/opt/wasi-sdk` otherwise) lacks the wasi-sdk toolchain | `beni-sys` build fails and names the root |
| The wasi-sdk root in effect differs from the one the archive's sidecar records, or the sidecar records none | `beni-sys` build fails and names the roots it has |
| Bindings a `beni-sys` build uses that declare no integer width, no float, or a fixed-size GC arena | `beni-sys` build fails and names the bindings it read |
| A `beni` build that receives no integer-width metadata or no float-width metadata | `beni` build fails and names the metadata it expected |
| `Mrb::open` failing to produce an interpreter | returns an error, never aborts |
| An exception raised by a raw binding inside a `sys::protect` body | surfaced as a Rust `Err` carrying the exception, the pending exception cleared from the handle; never unwinds past the caller |
| An allocation the interpreter cannot satisfy, inside a typed operation | mruby's out-of-memory raise, outside every total or never-raising statement. An operation that surfaces `Err` surfaces it as one carrying that exception; inside one that surfaces none — a conversion into a value, a read or render stated to never raise — it reaches the nearest mruby frame that rescues it, and ends the process where none does |
| A typed array, hash, or string mutated through a frozen receiver, an instance-variable or constant assignment or removal, or a class-variable assignment, to a frozen receiver, or a class-variable read resolving to no class variable | surfaced as a Rust `Err`, never unwinds across FFI |
| A Ruby method invoked through a value's dispatch, an object `dup` running `initialize_copy` or string coercion running `to_s`, an array join rendering an element via `to_s`, an instance construction running `initialize`, a constant fetch running a `const_missing` hook or resolving to no constant, a constant assignment running a `const_added` hook, a hash read / assignment / fetch / key test / deletion / merge running a key's `hash`/`eql?`, or a hash read running an absent-key `default` lookup, raising; a current-fiber read finding `Fiber` naming no class, or `Fiber.current` raising or answering anything but a Fiber | surfaced as a Rust `Err`, never unwinds across FFI |
| A numeric conversion of a non-numeric value, or of an infinite / NaN float to integer, or a String-tag coercion of a value carrying no String tag | surfaced as a Rust `Err`, never unwinds across FFI |
| A name of `UINT16_MAX` bytes or more given to a creating intern, to a string's coercion to a symbol, or as a symbol-or-name key | surfaced as a Rust `Err` carrying the `ArgumentError`, never unwinds across FFI; an operation that reports no failure answers instead as it does for a name nothing is bound under — a predicate `false`, a read `nil`, a removal a no-op |
| A class whose instances are neither plain objects nor data carriers — a singleton class, or a class whose instances have a built-in layout such as an exception, a string, or a number — marked to carry Rust data | surfaced as a Rust `Err` carrying a `TypeError`; the class stays unmarked |
| A `TypedData` value wrapped as an instance of a class that cannot carry data — one never marked, breaking the `TypedData` contract | the payload not yet handed to a carrier is reclaimed, never leaked, and the wrap panics; nothing unwinds across FFI |
| A `TypedData` type's carriers marked in an interpreter where a path resolves to no class, resolves to a value that is not a class, or names a class that refuses the mark | surfaced as a Rust `Err`; whichever classes it marked before the failure stay marked and held |
| An `InlineStruct` type's class marked in an interpreter where the path resolves to no class or to a value that is not a class, or names a class whose instances are not plain objects and that does not belong to the same type | surfaced as a Rust `Err`; the class stays unmarked |
| An `InlineStruct` value wrapped as an instance of a class whose instances are not inline structs — one never marked, breaking the `InlineStruct` contract | the wrap panics; nothing unwinds across FFI |
| A value converted to an inline struct of a type it is not, or an inline struct's payload replaced while it is frozen | surfaced as a Rust `Err` carrying the `TypeError` or `FrozenError`, never unwinds across FFI |
| A macro-implemented `TypedData` type naming a class — through a wrap or `TypedData::class` — whose path the interpreter's carrier record does not hold, or a macro-implemented `InlineStruct` type naming its class while the record holds none for it | panics, naming the `mark_carriers` call that records it; a value being wrapped is dropped, never leaked, and nothing unwinds across FFI |
| Ruby's `new` or `allocate` on a class whose default allocator is undefined | raises mruby's `TypeError` "allocator undefined for *class*"; reached through the typed surface, surfaced as a Rust `Err` |
| A `wrap` or `TypedData` derive missing `class`, given an attribute the macros do not accept, a value holding a NUL byte, or a `class` path holding an empty segment, or applied to a type with generic parameters or lifetimes | a compile error naming the offending attribute, value, or generics; nothing is generated |
| An `InlineStruct` derive or `wrap(inline)` applied to an enum, a union, or a type with generic parameters or lifetimes, given an attribute it does not accept, or applied to a type that is not `bytemuck::Pod` or exceeds three pointer widths in size or a pointer's alignment | a compile error; nothing usable is generated |
| Installing user data into an interpreter whose slot already holds a value | refused; the offered value handed back and the held value unchanged |
| Installing a payload into a data carrier that already holds one | refused; the offered payload handed back and the held payload unchanged |
| A hash mutated through its own iterate closure re-entering the VM, raising mruby's in-walk `RuntimeError` | surfaced as a Rust `Err`, never unwinds across FFI |
| Dumping a Proc backed by a C function, or a dump mruby cannot complete | surfaced as a Rust `Err` carrying an exception, no bytes produced |
| A precompiled bytecode blob the interpreter cannot read as a program | surfaced as a Rust `Err` carrying a `ScriptError` whose message names which structural check failed; nothing runs |
| A precompiled bytecode program raising while it runs | surfaced as a Rust `Err` carrying the exception, the pending exception cleared from the handle |
| A block invoked through `Proc::call` exiting via a non-local `break` or `return` | the escaping mruby break object surfaces as a Rust `Err`, inspectable as a typed break view; beni does not classify the exit into an outcome |
| Creating a compile context against a live interpreter failing | returns no context, never aborts |
| A codegen step failing, or the program raising while it runs, under a compile context | surfaced as a Rust `Err` carrying the exception, the pending exception cleared from the handle, never unwinds across FFI |
| A load or compile under a compile context whose filename is `UINT16_MAX` bytes or more | surfaced as a Rust `Err` carrying the `ArgumentError`, the pending exception cleared from the handle, never a parse message; creating the context succeeds |
| Source that does not parse | surfaced as a Rust `Err` carrying a parse message with the first recorded diagnostic's line, column, and text; nothing written to standard error |
| Source that does not parse, the compiler having recorded no diagnostic | surfaced as a Rust `Err` carrying a parse message with zero line, zero column, and empty text |
| Allocating the context a load borrows failing | the load surfaces as a Rust `Err` carrying a parse message with zero line, zero column, and empty text, never aborts |
| A load under a caller's compile context producing compiler warnings | the load's outcome is unchanged; the context answers the warnings as parse messages |
| A load under a borrowed compile context producing compiler warnings | the load's outcome is unchanged; the warnings reach no caller, and none are written to standard error |
| Compiling source without running it, where the source does not parse or a codegen step fails | the same `Err` a load that runs surfaces, and no compiled program is produced |
| A consumer whose code calls an operation the `compiler` or `fiber` feature carries, against an archive built without that feature's gem | both crates build, and the consumer's own link fails on the symbols the archive does not carry |
| A fiber created from a `Proc` backed by a C function; resumed when finished, running, already resumed, transferred to, or never initialized; or tested for liveness when never initialized | surfaced as a Rust `Err` carrying mruby's `FiberError`, never unwinds across FFI |
| A fiber's block raising while a resume runs it | surfaced as a Rust `Err` carrying the exception; the resumer's fiber is current again and the interpreter stays usable |
| A registered method returning a fiber yield outside a resumed fiber, or with a call from C or Rust code into Ruby between the fiber's block and the method | mruby's `FiberError` raised to the method's Ruby caller; no fiber switches |
| A class defined under a name bound to anything but an ordinary class with the given superclass, or mruby raising during class or module definition, method registration, method aliasing, method undefinition or removal, or module inclusion or prepend (including a cyclic include or prepend) | surfaced as a Rust `Err`, never unwinds across FFI |
| Rust panic raised inside any closure the safe wrapper invokes (`Gem::init` body, registered method, Rust-defined proc body, a closure run through `sys::catch_unwind`) | caught at the FFI boundary; surfaced as a Rust `Err` to the Rust caller (`Gem::init` body, `sys::catch_unwind`) or as an mruby exception to the caller (registered method, Rust-defined proc body); never unwinds into mruby's C frames |
| A Rust-defined proc's closure called again, through its proc or a copy of it, while it is already running | `RuntimeError` raised to the second caller; the running call is unaffected |
| A warning message longer than the longest string the interpreter holds, in an archive built with standard I/O | surfaced as a Rust `Err` carrying mruby's `ArgumentError`; nothing is written |
| Rust panic raised inside a `sys::protect` body | the process aborts at the FFI boundary; never unwinds into mruby's C frames |
| Registered method whose receiver or argument fails `TryConvert` conversion | the exception the conversion's `Err` carries raised to the Ruby caller, the closure body never runs |
| A registered method body's scan, single-argument, or named keyword read that the call does not fit — a wrong positional count, an argument or keyword value of the wrong type, a missing required block, a missing required keyword, or an unnamed keyword with no rest to collect it | surfaced to the body as a Rust `Err` carrying the exception raised for the mismatch; nothing raises past the body |
| A heap region buffer too small to hold one heap page | no pages are added and the count answers zero; the interpreter keeps allocating as before |
| `Gem::init` returns `Err` | interpreter setup aborts, the error surfaces to the embedder |

## Terminology

| Term | Meaning |
|---|---|
| symbol-or-name key | a name keying an operation, resolved to the `Id` it names: a string interns to it, and an already-interned `Id` or `Symbol` is reused as-is; a string key is a NUL-terminated C string, keyed on the bytes before its first NUL, or a Rust string, keyed on all of its bytes — beni's mirror of `magnus`'s `IntoId` |
| toolchain | a vendored build dependency (mruby source, wasi-sdk) |
| compile context | a filename stamp and top-level local variable scope shared by every load compiled through it; a program compiled under a filename-stamped one raises exceptions carrying a source-line backtrace. A load given no context borrows an unnamed one for its own duration |
| parse message | the line, column, and message text beni reports one compiler diagnostic in — an error or a warning; a failure the compiler recorded no diagnostic for is reported in the same shape |
| exception class | `Exception` itself or an ordinary class descending from it — never a singleton class — so every instance it allocates is an exception; the class an `ExceptionClass` handle names |
| carrier record | one interpreter's record of the class each `class` path of a macro-implemented `TypedData` type was marked as, and of each class an `InlineStruct` type was marked as together with the type it belongs to and the class `mark_carriers` last prepared for it; `mark_carriers` writes it and every naming of such a class, and every conversion to an inline struct, reads it |
| instance-variable holder | an object mruby lets hold instance variables — a plain object, a class or module (a singleton class included), a hash, a data carrier, or an exception; reached through `RObject`, `RClass`, `RModule`, `ExceptionClass`, `RHash`, `RTypedData`, `Obj<T>`, or `Exception` |
| inline struct | an object in mruby's `ISTRUCT` layout, holding up to three pointer widths of plain data inside the object itself — no heap payload, no release hook, no instance variables |
| hidden instance variable | an instance variable whose name does not begin with `@`, which no Ruby program can read, write, list, or remove; only a caller of the embedder API reaches it |
| plain object | an instance in the ordinary object layout `Object` and `BasicObject` give their instances, rather than a built-in type's own layout (an exception, a string, a number, …) or a data carrier's; a class allocates its instances in the layout its superclass allocated in when the class was defined |
| target declaration | a `target <name>` entry in the Rakefile block — names one build target to verify; its own block holds the target's toolchain references |
| toolchain reference | a block-less `toolchain <name>` inside a target declaration's block — requests the named toolchain for vendoring |
| toolchain definition | a top-level `toolchain <name>` block carrying `version` and `sha256` — replaces the named toolchain's built-in pair |
| built-in pair | the version and checksum pair the installed beni release vendors for a toolchain; a toolchain released as one tarball per build platform vendors one checksum per tarball, the pair carrying the build platform's |
| build platform | the CPU architecture and operating system the Rake tasks run on; it selects which of a toolchain's per-platform tarballs is downloaded |
| vendor tree | the directory tree the `vendor_dir` setting names |
| tarball cache | downloaded toolchain tarballs, kept inside the vendor tree |
| archive | the built mruby static library for one target; its file name is the one the toolchain that built it gives, and its compile-flags sidecar names it by that name |
| discovered archive | the archive located by archive discovery for the active cargo target |
| archive discovery variable | `MRUBY_LIB_DIR` or `BENI_VENDOR_DIR`, the environment variables archive discovery consults |
| staged | present in the vendor tree and ready to consume — toolchains unpacked, archives built |
| staged path | `mruby/build/<name>/lib/` under the vendor tree, holding one target's archive and compile-flags sidecar |
| wasi toolchain file | `tasks/toolchains/wasi.rake` under the staged mruby source — beni's wasm32-wasip1 cross-compile settings, staged whenever `wasi-sdk` is selected and activated by a build config via `conf.toolchain :wasi` |
| compile-flags sidecar | `libmruby.flags.mak`, the per-archive record of the archive's file name, the compiler that built it, the flags that compiler was given, and the libraries it needs linked |
| configured integer width | the bit width of mruby's integer the bindings a build uses declare — 32 or 64; mruby settles it from the archive's flags and the target, and the documentation bindings carry the 64-bit width of the upstream default configuration |
| integer-width metadata | the `links` metadata key `defines_mrb_int64` the `beni-sys` build publishes, reaching a direct dependent's build as `DEP_MRUBY_DEFINES_MRB_INT64` — `true` for a 64-bit configured integer width, `false` for a 32-bit one |
| configured float width | the bit width of mruby's float the bindings a build uses declare — 32 or 64; mruby settles it from the archive's flags, and the documentation bindings carry the 64-bit width of the upstream default configuration |
| float-width metadata | the `links` metadata key `defines_mrb_float32` the `beni-sys` build publishes, reaching a direct dependent's build as `DEP_MRUBY_DEFINES_MRB_FLOAT32` — `true` for a 32-bit configured float width, `false` for a 64-bit one |
| supported mruby floor | mruby 4.0 — the oldest release the crates build against; an archive states its own version in the header tree staged beside it |
| documentation host | the service that renders a published crate's documentation from the registry, without network access or a place to stage an archive; it announces itself to a build script through the `DOCS_RS` environment variable and builds on one platform, `x86_64-unknown-linux-gnu` |
| documentation build | a build the documentation host runs, told by that variable alone: nothing else marks a build as one, and nothing else unmarks it. It renders documentation and never links, so declarations are the whole of what it needs from `beni-sys` |
| documentation bindings | `bindings_docs.rs`, the bindings a documentation build reads in place of a discovered archive's. Generated from an mruby built with the upstream default configuration, and carrying what the generating host decides alongside it — type widths, the form of `va_list`, the constants its headers define. Never hand-written and never tracked by the repository: the published package carries the copy a release generated, and every other copy is generated where it is read |
| root | a hold that keeps a value reachable for the collector independently of the arena and of any Ruby reference to it — released when its holder is dropped, or never when registered for the interpreter's lifetime |
| heap region | a caller-owned byte buffer handed to the collector to carve into heap pages, owned by the caller for the process's lifetime and never freed by mruby |
| capability feature | a cargo feature on the `beni` crate carrying a capability mruby keeps in a gem rather than its core — declared by the consumer rather than probed from the archive, enabled by default, and additive, so enabling one only adds surface |
| dependency feature | a cargo feature on the `beni` crate carrying the conversions to a third-party Rust crate's types, disabled by default and additive, so enabling one only adds surface |
| library-internal header | a header mruby stages beside an archive while marking it internal to the library; its declarations are not embedder API, and the typed surface reaches them only under the admission rule |
| admitted internal symbol | a library-internal header's symbol the typed surface carries because no published symbol delivers its capability — admitted one at a time, recorded with what settles it, and never re-exported raw |
| declined symbol | public embedder API the typed surface deliberately does not carry, outside the coverage measure and recorded with what settles it |
| flag-gated symbol | embedder API a build's ABI lacks because a compile-time flag gates it, outside the coverage measure and recorded with what settles it — the gating flag |
