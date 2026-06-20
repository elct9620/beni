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

#### Handle, values, and conversions

The crate owns every Rust-level abstraction over the C API: an RAII interpreter
handle (`Mrb`, opened via `Mrb::open`), `Value` newtypes, class and module
definition, and closure-based exception protection. Two typed conversions cross
the Rust/Ruby boundary:

| Conversion | Direction | Rule |
|---|---|---|
| `IntoValue` | Rust value → `Value` | total — cannot fail |
| `FromValue` → `RString` / `Array` / `Hash` / `RClass` / `Proc` / `Symbol` / `Range` | `Value` → typed handle | converts on the target's type tag (subclass instances included for strings and containers); any other tag rejects |
| `FromValue` → `bool` | `Value` → `bool` | Ruby truthiness — `nil` and `false` to `false`, every other value to `true`; total, never rejects |

A value also converts to an `RString` handle by the same String type tag, but
surfacing the mismatch as an `Err` rather than rejecting to `None`: it succeeds
with the handle on a String tag and surfaces a `TypeError` on any other tag. It
runs no user Ruby — it dispatches no `to_str` — so it is the raising counterpart
to the `FromValue` → `RString` downcast, not the dispatching `to_s` string
coercion. The downcast suits a handler that treats a non-String as absent; the
raising form suits one that requires a String argument and rejects anything else.

Every type tag also carries a per-type predicate (`Value::is_array`,
`is_string`, `is_integer`, … — the analogue of mruby's `mrb_*_p` macros). Where
a tag has a typed handle, its predicate and `FromValue` downcast — magnus's
`TryConvert` analogue — agree exactly: the predicate holds for precisely the
values the downcast accepts. The predicate answers "what type is this?"; the
downcast hands back the handle to operate on it.

#### Strings

Rust bytes convert to a new mruby string, returned as a typed `RString`; a
string also constructs empty with a preallocated capacity, the buffer Ruby's
`String.new(capacity:)` reserves for appends that follow. A string also
constructs over a borrowed static buffer without copying its bytes — the no-copy
counterpart of the copying conversion, where the string aliases the caller's
bytes instead of owning a copy. The borrowed buffer must stay valid for the whole
run of the program (a `'static` requirement the construction enforces, making a
dangling alias impossible), since mruby never frees it; mruby treats such a string
copy-on-write, so an in-place append or resize reallocates first and then behaves
like any other string. magnus has no direct analogue, so this construction anchors
on mruby's own `mrb_str_new_static`, with `mrb_str_new_lit` the convenience that
borrows a string literal. From an mruby string Rust reads the bytes three ways:

| Read | Yields | Rejects |
|---|---|---|
| borrowed slice | a byte view of the string | — |
| owned `String` | the bytes when valid UTF-8 | a non-string tag, or non-UTF-8 bytes |
| owned `Vec<u8>` | arbitrary bytes | a non-string tag |

The three reads above never raise. Rust also reads the bytes two fallible ways.
It reads them as a NUL-terminated C-string view — the bytes guaranteed to end in
a `\0`, suitable for a C boundary — which surfaces an `Err`, the `ArgumentError`
mruby raises, when the bytes contain an embedded NUL, because a C string cannot
carry an embedded NUL. magnus offers no direct C-string accessor, so the read
anchors on mruby's own `mrb_string_cstr`. It also parses the bytes to an integer
in a given radix — a strict parse that rejects any non-integer input rather than
stopping at the first invalid character — which surfaces an `Err`, the
`ArgumentError` mruby raises, when the bytes are not a valid integer in that
radix. The radix is one of 2 through 36, or 0 to auto-detect a leading base
prefix (`0x`, `0b`, `0o`), the same radixes Ruby's `String#to_i` accepts; a radix
outside that domain is itself invalid input and surfaces the same `Err`. This
parse anchors on mruby's own `mrb_str_to_integer`; it is the strict counterpart
of Ruby's lenient `String#to_i`, which never raises. It likewise parses the bytes
to a float — a strict parse that rejects any non-float input rather than ignoring
trailing characters — which surfaces an `Err`, the `ArgumentError` mruby raises,
when the bytes are not a valid float. This parse anchors on mruby's own
`mrb_str_to_dbl`; it is the strict counterpart of Ruby's lenient `String#to_f`,
which never raises.

The inverse direction renders an Integer value to a new `RString` in a given
radix, the way Ruby's `Integer#to_s(base)` does — `12345` to `"3039"` in base 16.
The radix is one of 2 through 36; a radix outside that domain surfaces an `Err`,
the `ArgumentError` mruby raises. The render guards its receiver on the Integer
tag rather than trusting it, so a non-Integer value surfaces an `Err` carrying a
`TypeError` instead of reading a malformed value. magnus offers no direct radix
render, so this anchors on mruby's own `mrb_integer_to_str`.

A Float value converts to the Integer value it truncates toward zero, the way
Ruby's `Float#to_i` / `Float#to_int` core does — `3.9` to `3`, `-3.9` to `-3`.
The conversion guards its receiver on the Float tag, so a non-Float value
surfaces an `Err` carrying a `TypeError`; an infinite or NaN float has no integer
and surfaces an `Err` carrying a `RangeError`. This stays in mruby's value domain
— a Float value to an Integer value, not a value to a Rust scalar — and anchors
on mruby's own `mrb_float_to_integer`.

A registered method grows an `RString` in place by appending Rust bytes,
appending another mruby string's bytes, or appending a NUL-terminated C string's
bytes — its content up to the terminating NUL, the C-boundary counterpart of the
byte append, anchored on mruby's own `mrb_str_cat_cstr` — the way Ruby's
`String#<<` extends its receiver. It also appends any value coerced to a string,
the way Ruby's `String#concat` accepts a non-string argument — the dispatching
counterpart to the byte and string appends. Beyond reading and appending, a
string duplicates into an independent copy (Ruby's `String#dup`). It tests
another for byte equality (Ruby's `String#==`) and orders against another by byte
content (Ruby's `String#<=>`) — total reads that dispatch nothing and never
raise. It also interns its own bytes into the typed `Symbol` they name, creating
that symbol when it does not yet exist (Ruby's `String#intern`), dispatching
nothing and never raising. It also concatenates with another string
into a new string (Ruby's `String#+`), anchored on mruby's own `mrb_str_plus`:
the result is a freshly allocated string holding both operands' bytes, and
neither operand is mutated — the non-mutating counterpart of the in-place append,
which grows its receiver. With both operands already strings it dispatches
nothing and never raises, returning the new string directly rather than a
fallible result. A registered method also resizes a string's
length in place — truncating, or extending with undefined trailing bytes — and
reads a substring by character range, yielding the substring or nothing when the
range falls outside the string. It also searches for a substring, yielding the
byte index of the first match at or after a start offset, or nothing when the
substring is absent — a total read that dispatches nothing and never raises. A
negative offset counts from the string's end, an offset past the end finds
nothing, and an empty substring is found at the offset itself.

#### Symbols

A name interns into a typed `Symbol`, and an already-interned id reifies back
into one; the symbol reads its interned id back out. A name also interns over a
borrowed static buffer without copying its bytes — the no-copy counterpart of the
copying intern, where the interned name aliases the caller's bytes instead of
owning a copy; the borrowed buffer must stay valid for the whole run of the
program (a `'static` requirement the intern enforces), since mruby keeps the
pointer and never frees it. This intern anchors on mruby's own
`mrb_intern_static`, with `mrb_intern_lit` the convenience that borrows a string
literal.

Those interns all create the symbol when none exists yet. A name also checks for
an already-interned `Symbol` without creating one: the bytes resolve to the
symbol they name when mruby has interned it before, and to nothing when no such
symbol exists. The check dispatches nothing and never raises.

Where those interns take Rust bytes, an existing mruby value also coerces into a
typed `Symbol`: a symbol value yields its own id, a string value interns its
contents, and any other value surfaces an `Err` — the `TypeError` mruby raises
for a value that is neither a symbol nor a string. The coercion dispatches no
user Ruby; it follows the raise/return contract like the other converting
operations.

Beyond the id, a symbol
reads its name three ways into Rust-side views — all non-dispatching reads that
never raise, each yielding nothing when mruby has no name for the id:

| Read | Yields |
|---|---|
| name as `&str` | the name as UTF-8, escaped to its quoted dump form when it carries an embedded NUL |
| name as bytes | the raw name bytes with their true length, embedded NUL bytes included and unescaped |
| dump form | the name's symbol-literal representation, quoted and escaped when the name is not a plain identifier — Ruby's `Symbol#inspect` without the leading colon |

A symbol also reifies its name as an mruby String value, the way Ruby's
`Symbol#to_s` does — where the three reads above borrow into mruby's interned
storage, this returns a distinct, mutable `RString` whose bytes are the symbol's
name (unfrozen, unlike `Symbol#name`). It is the only symbol name read that
produces a value rather than a borrowed view; like the others it dispatches
nothing and never raises.

#### Ranges

A Range constructs from a begin value, an end value, and an exclusive-end flag,
mirroring Ruby's `Range.new(begin, end, exclusive)`; it surfaces an `Err` when
the two bounds cannot be compared — the `ArgumentError` mruby raises for a bad
range. A Range reads its begin value, its end value, and whether it excludes its
end — three non-dispatching reads that never raise.

#### Errors and the raise/return contract

A registered method or protected closure raises its own exception: it builds one
from an exception class and a message, or — when validating its own argument
count — from a given count and the expected minimum and maximum, yielding the
canonical `ArgumentError` ("wrong number of arguments (given N, expected …)")
mruby itself produces. Either way it returns the exception as an `Err`, which
crosses the boundary like any other `Err` — to a registered method's Ruby caller
as an mruby exception, to a protected closure's Rust caller as the `Err` value.

Every mutating or dispatching operation across the typed surface follows one
raise/return contract:

| Operation kind | Surfaces `Err` | Returns |
|---|---|---|
| Mutates a receiver — array append/remove/extend/replace/clear, indexed write and resize, hash assign/delete/merge/clear, string append and resize, instance-variable assignment and removal, class-variable assignment, constant assignment and removal | the receiver is frozen; an indexed write also when the index is out of range — a negative index past the beginning, or one too large; a string resize also when the requested length is negative or overflows; an instance-variable assignment also when the receiver cannot hold instance variables; a constant assignment or removal also when the receiver is not a class or module | `Result` |
| Dispatches Ruby — a method call, `==` / `eql?`, a `<=>` comparison, an object `dup` / `clone` or string coercion, a splat coercion to an array running a non-array's `to_a`, an array join rendering each element via `to_s`, an instance construction running `initialize`, a constant fetch running a `const_missing` hook, a constant assignment running a `const_added` hook, a hash read / assignment / fetch / key test / deletion / merge running a key's `hash` / `eql?`, a hash read running a `default` lookup for an absent key, or a range construction comparing its two bounds | the dispatched code raises; a splat coercion also when a `to_a` responder returns a non-array non-`nil` value; a constant fetch also when the name resolves to no constant; a range construction also when its two bounds cannot be compared | `Result` (a `<=>` comparison yields nothing when the two values are incomparable) |
| Reads a named variable that raises on absence — a class-variable read, walking the ancestry | the name resolves to no class variable | `Result` |
| Converts without dispatching — a numeric conversion across the numeric types, a Float value to the Integer value it truncates, or coercing a value to an `RString` / `Array` / `Hash` handle by its String / Array / Hash tag | the value is non-numeric (a non-Float receiver of the Float-to-Integer conversion raises a `TypeError`), or an infinite / NaN float converts to integer (a `RangeError`); the coerced value carries no String / Array / Hash tag | `Result` |
| Reads or renders without dispatching but can still raise — a string's NUL-terminated C-string view, a strict parse of a string to an integer in a given radix or to a float, or rendering an integer to a string in a given radix | the bytes contain an embedded NUL; the bytes are not a valid integer in the radix; the bytes are not a valid float; the render radix is outside 2 through 36, or its receiver is not an Integer | `Result` |
| Reads or examines without dispatching — indexed read, keys, values, size, emptiness, container duplication, substring read by character range, substring search by byte index, byte comparison, symbol name and dump reads, range begin / end / exclusive-end reads, instance-variable read and presence, class-variable presence, constant presence, `respond_to?`, `equal?`, `is_a?`, `instance_of?`, class, type predicate | never | a bare value, or the absent value when the substring range or an absent symbol name falls outside the read |

#### Containers

The typed array carries Ruby `Array`'s surface:

| Operation | Behavior |
|---|---|
| construct | empty, with a preallocated capacity, from a slice of values, or as a pair holding two given values |
| append | add a value to the end |
| indexed read | the element, or `nil` when the index is out of range |
| indexed write | Ruby's `ary[i] = v`, growing with `nil` to reach past the end |
| resize | set the length — growing with `nil` to reach a longer length, truncating to a shorter one |
| remove | take a value from either end |
| extend | append another array's elements |
| replace | make its contents a copy of another array's, in place — the receiver is mutated to hold those elements, not returned as a new array |
| join | the elements rendered into one string, separated by a given separator — each element's `to_s` runs and a raise inside it surfaces as an `Err`; an absent separator concatenates the renderings with nothing between them |
| clear | empty it |
| duplicate | copy it |

A typed hash constructs empty, or empty with a preallocated capacity that reserves room for the assignments that follow — the capacity is a hint, not content, and the hash starts empty. Beyond construction it carries Ruby `Hash`'s surface:

| Operation | Behavior |
|---|---|
| assign | set a key's value |
| read | the value, or `nil` when the key is absent; surfaces an `Err` when a key's `hash` / `eql?` or an absent-key `default` lookup raises |
| fetch | the value, or a supplied default when the key is absent, like Ruby's `Hash#fetch(key, default)` |
| key test | whether a key is present |
| delete | remove a key, returning its former value |
| merge | fold another hash into this one |
| clear | empty it |
| duplicate | copy it |
| keys / values | read as typed arrays |
| size / emptiness | the entry count, and whether it holds no entries |
| iterate | visit each key-value pair in insertion order, handing both to a closure that signals whether to continue or stop — stopping ends the walk before the remaining pairs. The walk dispatches no Ruby and surfaces no `Err`; mutating the hash from within the closure is unsupported. A closure panic stops the walk and resurfaces on the Rust side once the walk unwinds, never crossing into mruby's frames |

#### Value operations

| Operation | Semantics |
|---|---|
| `equal?` | object identity — the same object or not; a total predicate |
| `object_id` | a unique integer identifier for the value; dispatches nothing and never raises, a total operation |
| `==` / `eql?` | Ruby value and hash-key equality; may run a user-defined `==` or `eql?` |
| comparison | three-way order by Ruby's `<=>` — less, equal, or greater — yielding nothing when the two values are incomparable; may run a user-defined `<=>`, and a raise inside it surfaces as an `Err`. Distinct from equality: it ranks rather than tests sameness |
| dispatch | call a Ruby method named by a symbol-or-name key with an argument slice, receiving its return value |
| inspect | the value's debug string, Ruby's `inspect`; runs a user-defined `inspect`, and a raise inside it yields an empty string |
| `dup` / `clone` | copy the object, running its `initialize_copy` — `dup` resets the frozen state and drops the singleton class, `clone` preserves both; an immediate returns itself; may raise |
| string coercion | the value as a string — itself when already a string, otherwise its `to_s`; may raise when `to_s` does not return a string |
| numeric conversion | the value as a Rust integer or float, converted across the numeric types — to integer, an Integer reads directly and a Float truncates; to float, a Float reads directly and an Integer widens; surfaces an `Err` when the value is non-numeric, or when an infinite or NaN float converts to integer. Runs no user Ruby. Unlike the exact-tag `FromValue` downcast, which rejects any other tag outright, this converts between numeric types |
| splat coercion | the value spread to a new typed `Array`, Ruby's `*` coercion: an array yields a copy of itself; a non-array that responds to `to_a` runs it, taking the result when it is an array and wrapping the value in a one-element array when `to_a` returns `nil`; a value that answers no `to_a` wraps in a one-element array. Surfaces an `Err` when `to_a` raises or returns a non-array non-`nil` value. Unlike the tag-coercion to an `Array` handle, which dispatches nothing and takes only an already-array-tagged value, this runs `to_a` and always yields an array |
| `is_a?` | an instance of a class or any of its subclasses |
| `instance_of?` | a direct instance of a class |
| class | the class the value belongs to |
| freeze | freeze the value in place |
| frozen check | a precondition guard that surfaces an `Err` when the value is frozen — an immediate counts as frozen — and `Ok` otherwise; runs no user Ruby |
| instance variable | read a named instance variable — `nil` when unset — assign one in place, test its presence, or remove one and yield its former value; the read and presence test never raise; the assignment surfaces an `Err` when the receiver is frozen or cannot hold instance variables; the removal yields the former value, distinguishes an absent variable (and a receiver that cannot hold instance variables) from one removed while holding `nil`, and surfaces an `Err` only when a receiver that can hold instance variables is frozen |
| class variable | read a named class variable from a module or class, walking the ancestry, assign one in place on the module or class, or test its presence walking the ancestry; the read surfaces an `Err` when the name resolves to no class variable, the assignment surfaces an `Err` when the receiver is frozen, the presence test never raises |
| constant | fetch a named constant from a module or class, assign one in place, test its presence walking the ancestry, test its presence directly on the receiver alone, or remove one; the fetch surfaces an `Err` when the name resolves to no constant or its `const_missing` hook raises, the assignment surfaces an `Err` when the receiver is not a class or module, is frozen, or its `const_added` hook raises; the removal discards the former value, treats an absent constant as a no-op rather than an error, and surfaces an `Err` when the receiver is not a class or module or is frozen; both presence tests are total predicates that never raise, and the direct test answers true only for the receiver's own constant — never one inherited from an ancestor |
| `respond_to?` | whether the value answers to a named method; a total predicate |

#### Classes, modules, and methods

- Class and module definition are methods on the live `Mrb` handle:
  `define_class(name, superclass)` and `define_module(name)` return typed
  `RClass` and `RModule` handles. Methods are registered on those handles
  through the `Module` and `Object` traits (mirroring `magnus::Module` and
  `magnus::Object`), accepting Rust closures whose arguments and return
  values cross the boundary through `IntoValue` / `FromValue`; the `Module`
  trait also binds constants, aliases existing methods, mixes another module
  into the handle two ways — including it after the receiver in the ancestry
  (Ruby's `Module#include`, the receiver's own methods win) and prepending it
  ahead of the receiver (Ruby's `Module#prepend`, the module's methods override
  the receiver's own) — and undefines a method — Ruby's `Module#undef_method` —
  marking the name as not defined on the handle even when an ancestor defines
  it, with a class-method form that undefines a singleton method. A definition,
  registration, alias, module inclusion or prepend, or undefinition mruby
  rejects — including a cyclic include or prepend, or undefining a name absent
  from the handle and its ancestors — surfaces as a Rust `Err`.
- The live `Mrb` handle also creates an anonymous class — given a superclass —
  and an anonymous module, mirroring `magnus`'s anonymous class and module
  creation. The result is an unnamed `RClass` or `RModule` reachable only
  through the returned handle, never registered under a name in any namespace;
  it gains a name only when a consumer later binds it to a constant. Anonymous
  class creation surfaces a Rust `Err` when mruby rejects the superclass — a
  non-class, a singleton class, or `Class` itself; anonymous module creation
  always succeeds.
- Every definition and lookup keyed by a name — class, module, method, private
  method, module function, class method, constant, the class/module and
  built-in exception-class lookups on `Mrb` and the class/module lookups within
  a namespace, and method dispatch on a value — accepts the
  name as a symbol-or-name key, mirroring `magnus`'s `IntoId`: a string key
  interns to a symbol, an
  already-interned `Symbol` key is reused without re-interning. A consumer
  holding a `Symbol` reaches the definition or lookup without a redundant
  intern; the result is identical to passing the equivalent name, since both
  resolve to the same interned symbol.
- A built-in exception-class lookup on `Mrb` guarantees its result is a class
  descending from `Exception`: it surfaces a Rust `Err` when the name resolves
  to no constant, when the constant is not a class, and when the resolved class
  does not descend from `Exception`. This is the typed path to a built-in
  exception class — `RuntimeError`, `ArgumentError`, `TypeError` — for raising
  from registered code.
- The class/module lookup family also answers, as a total boolean predicate,
  whether a class or module is defined under a given name — top-level on the
  `Mrb` handle and within a namespace through the `Module` trait, both
  symbol-or-name keyed. Unlike the fetching lookups, the predicate never raises:
  a name bound in that scope answers `true`, an unbound one `false` rather than
  surfacing an `Err`. It is the precondition test a consumer runs before a
  fetching lookup that would otherwise raise on a missing name.
- A class handle resolves to its real class — its singleton-class and
  include-class links skipped — yielding the first user-facing class in the
  chain. A handle that is already a real class returns itself; the resolution
  walks the class structure and never raises. This is the named normalization a
  consumer reaches for after obtaining a handle that may be a singleton or
  include class (through the raw FFI seam); the value-level "class the value
  belongs to" already returns the real class, so it needs no separate
  resolution. The raw class of a value before that normalization — which may be
  a singleton or include class and demands VM-internal reasoning to use — stays
  behind `beni::sys`.
- A class or module handle reads its fully-qualified path — the namespace chain
  leading to it, `A::B::C` for a class nested under modules `A` and `B`, the bare
  name for a top-level handle. This is a total non-dispatching read that never
  raises, the read a consumer reaches for to render a handle by its place in the
  namespace. It yields nothing for an anonymous handle that has no place in any
  namespace. The path is distinct from the handle's unqualified name read: the
  name read always answers a name — synthesizing one for an anonymous handle —
  whereas the path read answers the qualified path or nothing, never a
  synthesized stand-in.
- A method registered for any arity reads its own call frame instead of
  receiving converted positionals: a shape-typed read projects the frame
  against a format marker into a typed tuple, a single-argument read returns
  the one required argument, and a count read returns the number of arguments
  passed. The single-argument read raises `ArgumentError` to the Ruby caller
  unless exactly one positional argument is present.
- A typed method registration declares a fixed count of required positionals
  and, after them, a count of optional positionals: each required positional
  crosses through `FromValue`, and each optional positional crosses as an
  `Option` of its type — present in the call binds `Some`, omitted binds
  `None`. Mirroring `magnus`'s trailing-`Option` arguments, the optional slots
  are the trailing parameters of the registered Rust function. The registration
  derives the argument-spec aspec from the two counts: required-only declares
  the required aspec, and a required-plus-optional declaration the
  required-and-optional aspec mruby uses to accept the optional positionals
  while still requiring the leading ones. A `FromValue` failure on a supplied
  argument — required or optional — raises to the Ruby caller before the body
  runs, as for the required-only form. Block arguments are not part of the
  typed arity model: a method that takes a block reads it through the
  `beni::sys` escape hatch.
- A registered method asks whether it was called with a block through a total
  predicate on the `Mrb` handle, mirroring magnus's `Ruby::block_given_p`: it
  reads the current call and answers `true` when a block was passed, `false`
  otherwise. It never raises. The predicate is a plain boolean question about
  the current call — it does not surface the call frame's block slot or any
  other VM-internal structure to the caller.
- Constructing an instance of a class handle runs Ruby's `Class.new` —
  allocating the object and running its `initialize` with an argument slice; a
  raising `initialize` surfaces as a Rust `Err`. Mirrors `magnus`'s
  `Class::new_instance`.
- A module function registers on a module handle in one call, becoming both a
  private instance method — for a class that mixes the module in — and a
  singleton method on the module object itself, the way `Math.sqrt` is callable
  as `Math.sqrt` and as a bare helper inside an including class. Class methods
  need no separate form: a singleton method defined on a class is its class
  method, mirroring magnus.
- A Rust-owned value backs an mruby object through the data-carrier
  mechanism (`CDATA`): a class is marked so its instances carry Rust data,
  a Rust value is wrapped as an instance of that class, and it is extracted
  back type-checked against the data type it was registered under — a value
  carrying a different data type, or none, does not extract. A bare carrier
  that holds no payload yet — the instance an mruby `dup` or `clone`
  allocates before `initialize_copy` runs — can have a Rust value installed
  into it. The install targets a bare carrier: it does not release any
  payload the carrier already holds, and on a value that carries no data
  type it does nothing — a total operation safe on any value. It is the
  seam through which a typed object copies its Rust state. The mruby
  garbage collector owns the wrapped value's lifetime, releasing it when
  its carrier is collected. Mirrors
  `magnus`'s typed-data wrapping, and meets the graduation bar — correct use
  needs no reasoning about VM internals — so it lives on the typed surface
  rather than behind `beni::sys`.

#### Gems, arena scopes, and blocks

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

#### Graduation, safety, and coverage

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
- `docs/api_coverage.md` measures how far the typed surface has graduated
  mruby's embedder API — the functions and macros an embedder calls across the
  public embedder headers. Compile-time and debug assertion macros and internal
  helper macros are not embedder API and stay out of the measure. A capability
  the typed surface graduates through a Rust-native construct rather than the
  matching C symbol counts as covered through that construct: a type predicate
  read from the value tag covers the per-type `_p` macro it stands in for, and
  a typed method definition's required and optional arity counts derive the
  argument-spec aspec it declares — the required, the required-and-optional, and
  the any-arguments aspecs.
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
| A typed array, hash, or string mutated through a frozen receiver, an instance-variable assignment or removal to a frozen receiver — assignment also when the receiver cannot hold instance variables, a class-variable assignment to a frozen receiver, or a constant assignment or removal to a frozen receiver or one that is not a class or module | surfaced as a Rust `Err`, never unwinds across FFI |
| A Ruby method invoked through a value's dispatch, an object `dup` / `clone` running `initialize_copy` or string coercion running `to_s`, an array join rendering an element via `to_s`, an instance construction running `initialize`, a constant fetch running a `const_missing` hook or resolving to no constant, a constant assignment running a `const_added` hook, a hash read / assignment / fetch / key test / deletion / merge running a key's `hash`/`eql?`, or a hash read running an absent-key `default` lookup, raising | surfaced as a Rust `Err`, never unwinds across FFI |
| A numeric conversion of a non-numeric value, or of an infinite / NaN float to integer, or a String-tag coercion of a value carrying no String tag | surfaced as a Rust `Err`, never unwinds across FFI |
| A block invoked through `Proc::call` exiting via a non-local `break` or `return` | the escaping mruby break object surfaces as a Rust `Err`, inspectable as a typed break view; beni does not classify the exit into an outcome |
| mruby raising during class or module definition, method registration, method aliasing, or module inclusion or prepend (including a cyclic include or prepend) | surfaced as a Rust `Err`, never unwinds across FFI |
| Rust panic raised inside any closure the safe wrapper invokes (`Gem::init` body, registered method, exception-protected closure) | caught at the FFI boundary; surfaced as a Rust `Err` to the Rust caller (`Gem::init` body, exception-protected closure) or as an mruby exception to the Ruby caller (registered method); never unwinds into mruby's C frames |
| Registered method receiving an argument that fails `FromValue` conversion | raised as an mruby exception to the Ruby caller, the closure body never runs |
| A registered method body's single-argument read receiving other than one positional argument | raised as an `ArgumentError` to the Ruby caller |
| `Gem::init` returns `Err` | interpreter setup aborts, the error surfaces to the embedder |

## Terminology

| Term | Meaning |
|---|---|
| symbol-or-name key | a definition or lookup name given either as a string, which interns to a symbol, or as an already-interned `Symbol`, reused as-is — beni's mirror of `magnus`'s `IntoId` |
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
