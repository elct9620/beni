/*
 * wrapper.h — bindgen entry point for the beni-sys crate.
 *
 * Pulled in by `build.rs::run_bindgen` to expose the mruby C API
 * surface the `beni` wrapper builds on. No hand-written C translation
 * units live in the crate any more: the static inline wrappers
 * below are the entire C surface, and bindgen's `wrap_static_fns`
 * emits a single trampoline file from them.
 *
 * The `<stdbool.h>` and `<sys/select.h>` pre-includes are not used
 * by the mruby surface itself — they cover bindgen's `wrap_static_fns`
 * trampoline file. bindgen emits a trampoline for every `static inline`
 * function reached through the include tree, including wasi-libc
 * helpers like `FD_ISSET`; the generated trampoline file `#include`s
 * only this wrapper, so `bool` and `fd_set` must resolve here even
 * though the safe layer never calls those helpers.
 */

#include <stdbool.h>
#include <sys/select.h>

#include <mruby.h>
#include <mruby/array.h>
#include <mruby/class.h>
#include <mruby/data.h>
#include <mruby/dump.h>
#include <mruby/error.h>
#include <mruby/hash.h>
#include <mruby/irep.h>
#include <mruby/numeric.h>
#include <mruby/proc.h>
#include <mruby/range.h>
#include <mruby/string.h>
#include <mruby/value.h>
#include <mruby/variable.h>

/*
 * Static inline wrappers around mruby macros that lack a public
 * MRB_API / MRB_INLINE counterpart. bindgen's `wrap_static_fns`
 * picks these up and emits real extern symbols Rust can call, so
 * macro expansion stays inside the C compiler (which knows the
 * per-build word-box / string layout) rather than being mirrored
 * in Rust.
 */

/* Raw byte pointer into a String-tagged mrb_value. Counterpart to
 * the `RSTRING_PTR(s)` macro from <mruby/string.h>; the macro
 * branches between the embed buffer and the heap pointer based on
 * the RString header flags, which bindgen cannot read directly. */
static inline const char *
mrb_rstring_ptr(mrb_value s)
{
  return (const char *)RSTRING_PTR(s);
}

/* Byte length of a String-tagged mrb_value. Counterpart to
 * `RSTRING_LEN(s)`; same embed-vs-heap branch as `mrb_rstring_ptr`. */
static inline mrb_int
mrb_rstring_len(mrb_value s)
{
  return RSTRING_LEN(s);
}

/* Element count of an Array-tagged mrb_value. Counterpart to the
 * `RARRAY_LEN(a)` macro from <mruby/array.h>, which branches between
 * the embedded-buffer length and the heap length on the RArray header
 * flags — the same embed-vs-heap branch as `mrb_rstring_len`, which
 * bindgen cannot read directly. */
static inline mrb_int
mrb_rarray_len_func(mrb_value a)
{
  return RARRAY_LEN(a);
}

/* Object pointer extractor from an object-tagged mrb_value.
 * Counterpart to the `mrb_obj_ptr(v)` macro in <mruby/value.h>,
 * which expands via `mrb_val_union(v).p`. Folding the union read
 * into a single C function sidesteps the wasm32 union-return ABI
 * mismatch bindgen's trampoline would otherwise hit. */
static inline struct RObject *
mrb_obj_ptr_func(mrb_value v)
{
  return mrb_obj_ptr(v);
}

/* Class pointer extractor from a class-tagged mrb_value.
 * Counterpart to the `mrb_class_ptr(v)` macro in <mruby/value.h>.
 * The pointer read depends on the boxing mode, so it must expand in
 * the C compiler — which sees the same defines libmruby.a was built
 * with — rather than being mirrored as a Rust-side word read. */
static inline struct RClass *
mrb_class_ptr_func(mrb_value v)
{
  return mrb_class_ptr(v);
}

/* Float unbox. Counterpart to the `mrb_float(o)` macro, whose
 * expansion differs per boxing mode (inline-rotated word, RFloat
 * heap read, NaN payload). Routing through the C compiler keeps the
 * unbox correct for whatever config libmruby.a was built with. */
static inline mrb_float
mrb_float_func(mrb_value v)
{
  return mrb_float(v);
}

/* Symbol unbox. Counterpart to the `mrb_symbol(o)` macro, whose
 * expansion differs per boxing mode (word-shifted payload vs. union
 * field). Routing through the C compiler keeps the unbox correct for
 * whatever config libmruby.a was built with. Pair with the
 * `mrb_symbol_value` MRB_INLINE (box direction), which needs no shim. */
static inline mrb_sym
mrb_symbol_func(mrb_value v)
{
  return mrb_symbol(v);
}

/* GC arena bracketing helpers. mruby exposes these as macros that
 * read / write `mrb->gc.arena_idx`; bindgen treats `mrb_gc` as
 * opaque (workaround for the bitfield mis-pack on wasm32, see
 * `build.rs::run_bindgen`), so reaching the field from Rust
 * requires routing through the C compiler. */
static inline int
mrb_gc_arena_save_func(mrb_state *mrb)
{
  return mrb_gc_arena_save(mrb);
}

static inline void
mrb_gc_arena_restore_func(mrb_state *mrb, int idx)
{
  mrb_gc_arena_restore(mrb, idx);
}

/* `mrb_proc_new` is declared in <mruby/proc.h> without `MRB_API`,
 * so the `-fvisibility=default` workaround in `build.rs` does not
 * make bindgen pick it up. The static archive still resolves the
 * symbol at link time; wrap it in a `static inline` here so
 * bindgen's `wrap_static_fns` emits a trampoline Rust can call. */
static inline struct RProc *
mrb_proc_new_func(mrb_state *mrb, const mrb_irep *irep)
{
  return mrb_proc_new(mrb, irep);
}

/* `mrb_nil_p(v)` expands differently across boxing configs
 * (word-box / NaN-box / no-box); reaching it from Rust must go
 * through the C compiler so we always read the layout libmruby.a
 * was built with. */
static inline mrb_bool
mrb_nil_p_func(mrb_value v)
{
  return mrb_nil_p(v);
}

/* `mrb_undef_p(v)` expands via `mrb_type(v) == MRB_TT_UNDEF`; the tag
 * read depends on the boxing config, so reaching it from Rust must go
 * through the C compiler to match libmruby.a's layout. Tells an
 * `mrb_undef_value()` sentinel — e.g. the absent-variable result of
 * `mrb_iv_remove` — apart from a real value. */
static inline mrb_bool
mrb_undef_p_func(mrb_value v)
{
  return mrb_undef_p(v);
}

/* `mrb_true_p(v)` / `mrb_false_p(v)` expand per boxing config: under
 * word-boxing they compare the immediate word against MRB_Qtrue /
 * MRB_Qfalse, while other boxings tag both `nil` and `false` as
 * MRB_TT_FALSE — so a Rust-side tag test would misread `nil` as
 * `false`. Route through the C compiler to match libmruby.a's layout. */
static inline mrb_bool
mrb_true_p_func(mrb_value v)
{
  return mrb_true_p(v);
}

static inline mrb_bool
mrb_false_p_func(mrb_value v)
{
  return mrb_false_p(v);
}

/* `mrb_test(v)` — Ruby truthiness: false only for `nil` and `false`.
 * Same boxing-config dependence as the predicates above. */
static inline mrb_bool
mrb_test_func(mrb_value v)
{
  return mrb_test(v);
}

/* RBreak predicate. Counterpart to `mrb_break_p(o)` in
 * <mruby/value.h>, which expands via `mrb_type(o) == MRB_TT_BREAK`.
 * Safe on any `mrb_value`; only reads the type tag. Yield helpers
 * use it to gate the `break` / Proc-`return` classification of a
 * value escaping a protected block call. */
static inline mrb_bool
mrb_break_p_func(mrb_value v)
{
  return mrb_break_p(v);
}

/* Read the `val` field of an RBreak-tagged `mrb_value`.
 *
 * Counterpart to the `mrb_break_value_get(brk)` macro in
 * <mruby/error.h> under beni's pinned word-boxing configuration
 * (which leaves `MRB_USE_RBREAK_VALUE_UNION` undefined, so the
 * macro resolves to a simple `brk->val` read).
 *
 * Caller must ensure `v` is RBreak-tagged via `mrb_break_p_func`;
 * behaviour is undefined otherwise. */
static inline mrb_value
mrb_break_value_func(mrb_value v)
{
  struct RBreak *brk = (struct RBreak *)mrb_obj_ptr(v);
  return mrb_break_value_get(brk);
}

/* Read the `ci_break_index` field of an RBreak-tagged `mrb_value`.
 * The index points at the destination callinfo frame mruby will
 * unwind to. Yield helpers compare it against the pre-yield baseline
 * from `mrb_current_ci_index_func` to discriminate a real `break`
 * (target ≥ baseline) from a non-orphan Proc `return`
 * (target < baseline). */
static inline uintptr_t
mrb_break_ci_index_func(mrb_value v)
{
  struct RBreak *brk = (struct RBreak *)mrb_obj_ptr(v);
  return brk->ci_break_index;
}

/* Current callinfo index: `mrb->c->ci - mrb->c->cibase`. Snapshotted
 * before the protected `mrb_yield_argv` call so the post-catch
 * comparison can place the RBreak's destination relative to the
 * yielder's frame. Public `mrb_context` fields per `<mruby.h>`
 * lines 196-202. */
static inline uintptr_t
mrb_current_ci_index_func(mrb_state *mrb)
{
  return (uintptr_t)(mrb->c->ci - mrb->c->cibase);
}

/* Argument-spec encoders. mruby spells MRB_ARGS_NONE() / MRB_ARGS_ANY()
 * / MRB_ARGS_REQ(n) as function-like macros in <mruby.h>; bindgen does
 * not expand macros, so wrap them here and let the C compiler emit the
 * `mrb_aspec` bit packing from mruby's own header rather than mirroring
 * it in Rust. Pure value computation — no mrb_state touched. */
static inline mrb_aspec
mrb_args_none_func(void)
{
  return MRB_ARGS_NONE();
}

static inline mrb_aspec
mrb_args_any_func(void)
{
  return MRB_ARGS_ANY();
}

static inline mrb_aspec
mrb_args_req_func(uint32_t n)
{
  return MRB_ARGS_REQ(n);
}

static inline mrb_aspec
mrb_args_arg_func(uint32_t req, uint32_t opt)
{
  return MRB_ARGS_ARG(req, opt);
}

static inline mrb_aspec
mrb_args_block_func(void)
{
  return MRB_ARGS_BLOCK();
}

/* The `mrb_undef_value()` sentinel, used to seed an optional-argument
 * out-parameter before `mrb_get_args`: an omitted optional leaves its
 * slot untouched, so the bridge reads back the undef tag to tell an
 * omitted argument from a supplied one. `mrb_undef_value` is a boxing-
 * dependent `MRB_INLINE` in <mruby/value.h>; bindgen does not expand
 * it, so the C compiler emits the sentinel for the layout libmruby.a
 * was built with. */
static inline mrb_value
mrb_undef_value_func(void)
{
  return mrb_undef_value();
}

/* Mark a class so its instances allocate as the given vtype.
 * Counterpart to the `MRB_SET_INSTANCE_TT(c, tt)` macro in
 * <mruby/class.h>, which rewrites the instance-type bits of the
 * class's `flags` word. bindgen cannot expand the macro, so the C
 * compiler packs the flags from mruby's own header rather than a
 * Rust-side mirror of the bit layout. A CDATA-backed class calls this
 * with `MRB_TT_CDATA` so instances allocate as data carriers. */
static inline void
mrb_set_instance_tt_func(struct RClass *c, enum mrb_vtype tt)
{
  MRB_SET_INSTANCE_TT(c, tt);
}

/* Integer conversion across the numeric types. Counterpart to the
 * `mrb_as_int(mrb, val)` macro in <mruby.h>, which expands to
 * `mrb_integer(mrb_ensure_int_type(mrb, val))`: an Integer reads directly
 * and a Float truncates. The conversion runs no user `to_int`; it raises
 * TypeError on a non-numeric value and RangeError on an infinite / NaN
 * float, long-jumping to the caller's protect frame. bindgen does not
 * expand the macro, so the C compiler folds the conversion and the
 * boxing-aware unbox using the layout libmruby.a was built with. */
static inline mrb_int
mrb_as_int_func(mrb_state *mrb, mrb_value val)
{
  return mrb_as_int(mrb, val);
}

/* Float conversion across the numeric types. Counterpart to the
 * `mrb_as_float(mrb, x)` macro in <mruby.h>, which expands to
 * `mrb_float(mrb_ensure_float_type(mrb, x))`: a Float reads directly and
 * an Integer widens. Runs no user `to_f`; raises TypeError on a
 * non-numeric value. Same boxing-aware unbox concern as
 * `mrb_as_int_func`. */
static inline mrb_float
mrb_as_float_func(mrb_state *mrb, mrb_value val)
{
  return mrb_as_float(mrb, val);
}

/* Begin value of a Range-tagged mrb_value. Counterpart to the
 * `mrb_range_beg(mrb, r)` macro in <mruby/range.h>, which reads
 * `RANGE_BEG` off the `struct RRange` the macro resolves via
 * `mrb_range_ptr`. The begin/end fields live inline or behind an
 * `edges` pointer depending on the boxing config (`MRB_RANGE_EMBED`),
 * so the C compiler must do the read against the layout libmruby.a
 * was built with. */
static inline mrb_value
mrb_range_beg_func(mrb_state *mrb, mrb_value range)
{
  return mrb_range_beg(mrb, range);
}

/* End value of a Range-tagged mrb_value. Counterpart to the
 * `mrb_range_end(mrb, r)` macro; same embed-vs-edges layout branch as
 * `mrb_range_beg_func`. */
static inline mrb_value
mrb_range_end_func(mrb_state *mrb, mrb_value range)
{
  return mrb_range_end(mrb, range);
}

/* Exclude-end flag of a Range-tagged mrb_value. Counterpart to the
 * `mrb_range_excl_p(mrb, r)` macro, which reads `RANGE_EXCL` off the
 * `struct RRange`; routed through the C compiler for the same layout
 * reason. */
static inline mrb_bool
mrb_range_excl_p_func(mrb_state *mrb, mrb_value range)
{
  return mrb_range_excl_p(mrb, range);
}
