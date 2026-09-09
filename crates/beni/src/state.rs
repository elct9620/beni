//! RAII wrapper around mruby's `mrb_state *` plus the per-concern
//! capability traits that extend it.
//!
//! `Mrb` owns a freshly opened mruby VM. `Mrb::open` allocates a
//! new state via `mrb_open`; `Drop` releases it via `mrb_close`.
//! Callers that still reach for the raw FFI use `Mrb::as_ptr` as an
//! explicit escape hatch.
//!
//! `Mrb` is `Send` and not `Sync`: an interpreter is carried between
//! threads, never reached from two at once. A consumer that wants two
//! threads to reach one interpreter wraps the handle in a `Mutex`, and
//! a guard that borrows the handle — an arena scope, a root, a compile
//! context — holds both on the thread that made it.
//!
//! ## Why a newtype rather than passing `*mut mrb_state`
//!
//! Two problems with the raw pointer:
//!
//! 1. Every function that takes one must be `unsafe fn` even when it
//!    does nothing more than forward to FFI — "unsafe contagion"
//!    across every helper that touches the VM.
//! 2. Manual `mrb_close` calls scatter across every error path of an
//!    embedder's eval entry point. Forgetting one is a quiet memory
//!    leak the type system cannot catch.
//!
//! `Mrb` fixes both: the owning type makes "the VM is live" provable
//! by the borrow checker, and `Drop` makes `mrb_close` automatic.
//!
//! ## Capability clusters
//!
//! The mruby C API surface the typed layer covers is grouped into
//! per-concern files under `state::`. Each file extends `Mrb` with
//! inherent methods covering one concern:
//!
//!   * `factory` — `String` / `Array` / `Hash` factories
//!   * `symbol` — symbol intern + name lookup
//!   * `define` — top-level module / class / const / gvar
//!   * `args` — `mrb_get_args` shape-typed dispatch via
//!     `Format` trait + `format`
//!     ZST markers (currently the only trait-based cluster — see the
//!     `args` module doc for the pattern, applicable to future
//!     clusters once combinatorial pressure shows up)
//!   * `load` — RITE bytecode loaders
//!   * `protect` — closure-based `mrb_protect_error`
//!   * `root` — GC roots outliving the frame that made the value
//!
//! Splitting per concern keeps each file's surface small and the
//! rustdoc on each cluster focused.

pub mod arena;
pub mod args;
pub mod define;
pub mod factory;
pub mod load;
pub mod protect;
pub mod root;
pub mod symbol;

use crate::{Error, RClass, Value};
use beni_sys as sys;
use core::ptr::NonNull;

/// Owning handle to a live mruby VM. Closed automatically on drop.
///
/// When mruby is linked the type is `#[repr(transparent)]` over
/// `NonNull<mrb_state>` so `Mrb::borrow_raw` can fabricate a `&Mrb`
/// reference from a raw `*mut mrb_state` received at a C-bridge
/// frame. The two layouts are byte-identical there.
///
/// An interpreter is carried between threads:
///
/// ```
/// fn carried<T: Send>() {}
/// carried::<beni::Mrb>();
/// ```
///
/// It is never reached from two at once, so sharing one across threads
/// is the consumer's own `Mutex` rather than something the handle
/// offers:
///
/// ```compile_fail
/// fn shared<T: Sync>() {}
/// shared::<beni::Mrb>();
/// ```
#[repr(transparent)]
pub struct Mrb {
    state: NonNull<sys::mrb_state>,
}

// SAFETY: an interpreter owns everything it runs on — heap, symbol
// table, arena — and mruby binds none of it to the thread that opened
// it, so an owner may hand one over. Every entry point that installs a
// jump target takes `&self`, so no move outruns a live one. `Sync` is
// withheld above: concurrent reach is the one thing an interpreter
// cannot survive, and refusing it is also what keeps `borrow_raw`'s
// `&Mrb` and the scope guards on the thread that made them.
unsafe impl Send for Mrb {}

/// Returned by `Mrb::open` when mruby could not produce a usable
/// interpreter: `mrb_open` returned NULL (allocation failure),
/// returned a state with a pending exception (core or gem init
/// failure).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MrbOpenError;

impl std::fmt::Display for MrbOpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("mruby could not produce a usable interpreter")
    }
}

impl std::error::Error for MrbOpenError {}

impl Mrb {
    /// Open a fresh mruby state. Returns `MrbOpenError` when mruby
    /// cannot produce a usable interpreter.
    pub fn open() -> Result<Self, MrbOpenError> {
        // SAFETY: `mrb_open` takes no arguments and returns an
        // owned state or NULL.
        let raw = unsafe { sys::mrb_open() };
        let Some(state) = NonNull::new(raw) else {
            return Err(MrbOpenError);
        };
        let mrb = Self { state };
        // `mrb_open` also signals failure by returning a state
        // with `mrb->exc` set — core or gem init failed (vendored
        // `src/state.c`). That state is not a usable interpreter;
        // dropping `mrb` here closes it.
        if !mrb.pending_exc().is_nil() {
            return Err(MrbOpenError);
        }
        Ok(mrb)
    }

    /// Raw `*mut mrb_state`. Use only at FFI boundaries that have
    /// not yet migrated to safe methods. The returned pointer is
    /// valid for the lifetime of `&self`; callers must not call
    /// `mrb_close` on it (the `Mrb` Drop owns that).
    #[inline]
    pub fn as_ptr(&self) -> *mut sys::mrb_state {
        self.state.as_ptr()
    }

    /// Borrow a live `*mut mrb_state` as an `&Mrb` reference. Used
    /// by C-bridge frames that receive a raw pointer from mruby and
    /// need to call the safe `Mrb` / capability-trait methods
    /// without first acquiring an owning `Mrb`.
    ///
    /// The returned reference does not own the state; no `mrb_close`
    /// runs when it goes out of scope. The owning `Mrb` (the one
    /// produced by `Mrb::open`) keeps Drop responsibility.
    ///
    /// ## Why a `&*mut mrb_state` parameter instead of a raw pointer
    ///
    /// `Mrb` is `#[repr(transparent)]` over `NonNull<mrb_state>`, so
    /// the *storage* of a `*mut mrb_state` variable has the same
    /// layout as an `Mrb` value. Taking a reference to that storage
    /// (`&*mut mrb_state`) and reinterpreting it as `&Mrb` is sound.
    ///
    /// Casting the pointer *value* itself (`mrb as *const Mrb`) is
    /// **not** equivalent: that produces a pointer to the bytes at
    /// address `mrb`, which are the first field of the `mrb_state`
    /// struct (`jmp: *mut mrb_jmpbuf`) — not an `Mrb` value containing
    /// the `mrb_state *` pointer. Reading through such an `&Mrb`
    /// would treat the `jmp` pointer as an `mrb_state *`, leading to
    /// silent UB and guest traps once any later mruby call dereferences
    /// the bogus state.
    ///
    /// # Safety
    ///
    /// `*mrb_ref` must be a live mruby state that remains open for
    /// the lifetime of the returned borrow. Passing storage holding
    /// NULL is undefined behaviour.
    #[inline]
    pub unsafe fn borrow_raw(mrb_ref: &*mut sys::mrb_state) -> &Mrb {
        debug_assert!(!mrb_ref.is_null());
        // SAFETY: `Mrb` is `#[repr(transparent)]` over
        // `NonNull<mrb_state>`, and `NonNull` is itself
        // `#[repr(transparent)]`
        // over `*mut mrb_state`. So a `*const *mut mrb_state` (the
        // address of the caller's pointer variable) and a `*const Mrb`
        // index into the same storage layout. The borrow lifetime is
        // inherited from `mrb_ref` via lifetime elision.
        unsafe { &*(mrb_ref as *const *mut sys::mrb_state as *const Mrb) }
    }

    /// Return the currently pending mruby exception, or
    /// `mrb_nil_value()` (`w == 0`) if none. Reads `mrb->exc`
    /// directly through the bindgen-exposed struct field; does NOT
    /// clear the field — callers pair this with `Mrb::clear_exc`
    /// after they have captured class/message/backtrace.
    pub fn pending_exc(&self) -> Value {
        // SAFETY: `self.state` is alive by the `&self` borrow. The
        // `exc` field is exposed by bindgen as `*mut RObject`; when
        // non-null it is the boxed exception's object pointer, which
        // `mrb_obj_value` reifies into the matching `mrb_value`.
        let exc = unsafe { (*self.state.as_ptr()).exc };
        if exc.is_null() {
            Value::from_raw(unsafe { sys::mrb_nil_value() })
        } else {
            Value::from_raw(unsafe { sys::mrb_obj_value(exc as *mut core::ffi::c_void) })
        }
    }

    /// What an operation that reached mruby answers: its value, or the
    /// exception it raised, cleared from the handle as it crosses out.
    /// Every load answers through here, so a raise reaches a Rust
    /// caller in one shape whatever compiled the program.
    pub(crate) fn outcome(&self, value: Value) -> Result<Value, Error> {
        let exc = self.pending_exc();
        if exc.is_nil() {
            Ok(value)
        } else {
            self.clear_exc();
            Err(Error::Exception(exc))
        }
    }

    /// Set `mrb->exc` to `exc`, replacing whatever was there. The slot
    /// a raise from inside a C bridge writes for itself, offered for a
    /// caller that has to stage an exception the VM did not raise.
    /// Most code paths should let mruby raise via `mrb_raise` instead;
    /// that path triggers the normal exception flow without needing a
    /// manual slot write.
    ///
    /// # Safety
    ///
    /// `exc` must be an exception-object-tagged `Value` originating
    /// from this VM: mruby's downstream machinery dereferences the
    /// slot as `RObject *`, so nil or any non-object value is
    /// undefined behavior on the next exception check.
    pub unsafe fn set_pending_exc(&self, exc: Value) {
        // SAFETY: `self.state` is alive by the `&self` borrow; `exc`
        // originates from the same VM. `mrb_obj_ptr_func` extracts the
        // RObject pointer carried by the value; the assignment installs
        // it as the new pending exception, replacing whatever sat in
        // the slot.
        let obj_ptr = unsafe { sys::mrb_obj_ptr_func(exc.into_raw()) };
        unsafe { (*self.state.as_ptr()).exc = obj_ptr };
    }

    /// Clear `mrb->exc`. Idempotent; safe to call when no exception
    /// is pending. Used by the consumer crate's panic-recovery paths
    /// after the pending exception has been extracted, so subsequent
    /// mruby calls do not observe stale exception state.
    pub fn clear_exc(&self) {
        // SAFETY: `self.state` is alive by the `&self` borrow. The
        // return value (a `mrb_bool` snapshot of the prior
        // `mrb->exc` state) is intentionally discarded.
        let _ = unsafe { sys::mrb_check_error(self.as_ptr()) };
    }

    /// Run one complete GC cycle, reclaiming every object unreachable
    /// from the live roots and the GC arena. Total: it returns nothing,
    /// never raises, and is safe whenever the VM is alive — a disabled or
    /// mid-collection collector ignores the request.
    pub fn full_gc(&self) {
        // SAFETY: `self.state` is alive by the `&self` borrow;
        // `mrb_full_gc` only triggers collection on it.
        unsafe { sys::mrb_full_gc(self.as_ptr()) };
    }

    /// Advance the incremental collector by a single step. Total: it
    /// returns nothing, never raises, and is safe whenever the VM is
    /// alive — a disabled or mid-collection collector ignores the request.
    pub fn incremental_gc(&self) {
        // SAFETY: `self.state` is alive by the `&self` borrow;
        // `mrb_incremental_gc` only advances collection on it.
        unsafe { sys::mrb_incremental_gc(self.as_ptr()) };
    }

    /// Hand the collector `buf` to carve into heap pages, so objects can
    /// live in memory the caller placed rather than only in pages the
    /// allocator hands out. Answers how many pages the buffer yielded,
    /// which is zero when it is too small to hold one.
    ///
    /// The buffer is taken by move for the process's whole lifetime: the
    /// caller cannot reach it again, and the same buffer cannot be handed
    /// over twice. mruby never frees it — closing the interpreter releases
    /// the descriptors it kept, never the memory behind them. Alignment
    /// within the buffer is the collector's concern, not the caller's.
    ///
    /// This adds to the collector's pages without capping them: once they
    /// are exhausted the collector grows through the allocator as it
    /// otherwise would.
    pub fn gc_add_region(&self, buf: &'static mut [u8]) -> usize {
        let len = buf.len();
        let start = buf.as_mut_ptr() as *mut core::ffi::c_void;
        // SAFETY: `self` is alive by the `&self` borrow. The buffer
        // is `'static` and moved in, so it outlives the VM and no
        // caller can write it again while the collector holds pages
        // carved from it.
        let pages = unsafe { sys::mrb_gc_add_region(self.as_ptr(), start, len) };
        usize::try_from(pages).unwrap_or(0)
    }

    /// Return `mrb->object_class` as a typed `RClass` handle.
    /// Replaces direct field access — the `object_class` field on
    /// the `crate::mrb_state` struct is `pub(crate)` so this
    /// accessor is the one external entry point. The free function
    /// `crate::mrb_object_class` remains for code paths that hold
    /// only a raw `*mut mrb_state`.
    #[inline]
    pub fn object_class(&self) -> RClass {
        // SAFETY: `self.state` is alive by the `&self` borrow.
        RClass::from_raw(unsafe { sys::mrb_object_class(self.as_ptr()) })
    }
}

impl Drop for Mrb {
    fn drop(&mut self) {
        // SAFETY: `state` was produced by `mrb_open` in `Mrb::open`
        // and has not been closed elsewhere — `as_ptr` hands out
        // borrows but never takes ownership.
        unsafe { sys::mrb_close(self.state.as_ptr()) };
    }
}
