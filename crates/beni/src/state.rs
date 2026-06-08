//! RAII wrapper around mruby's `mrb_state *` plus the per-concern
//! capability traits that extend it.
//!
//! `Mrb` owns a freshly opened mruby VM. `Mrb::open` allocates a
//! new state via `mrb_open`; `Drop` releases it via `mrb_close`.
//! Callers that still reach for the raw FFI use `Mrb::as_ptr` as an
//! explicit escape hatch.
//!
//! `Mrb` is intentionally `!Send` and `!Sync` (inherited from
//! `NonNull<mrb_state>`): mruby's `mrb_state` is single-threaded and
//! must not cross thread boundaries.
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
//!
//! Splitting per concern keeps each file's surface small and the
//! rustdoc on each cluster focused.

pub mod arena;
pub mod args;
pub mod define;
pub mod factory;
pub mod load;
pub mod protect;
pub mod symbol;

use crate::{RClass, Value};
use beni_sys as sys;
#[cfg(mruby_linked)]
use core::ptr::NonNull;

/// Owning handle to a live mruby VM. Closed automatically on drop.
///
/// In placeholder builds (no staged `libmruby.a`) the inner pointer
/// field is absent because `Mrb::open` always returns `Err`; the
/// type still compiles so that `Result<Mrb, MrbOpenError>` is a
/// uniform return type across builds.
///
/// When mruby is linked the type is `#[repr(transparent)]` over
/// `NonNull<mrb_state>` so `Mrb::borrow_raw` can fabricate a `&Mrb`
/// reference from a raw `*mut mrb_state` received at a C-bridge
/// frame. The two layouts are byte-identical there.
#[cfg_attr(mruby_linked, repr(transparent))]
pub struct Mrb {
    #[cfg(mruby_linked)]
    state: NonNull<sys::mrb_state>,
}

/// Returned by `Mrb::open` when mruby could not produce a usable
/// interpreter: `mrb_open` returned NULL (allocation failure),
/// returned a state with a pending exception (core or gem init
/// failure), or — in placeholder builds — was not linked at all.
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
    /// cannot produce a usable interpreter (or unconditionally in
    /// placeholder builds — no mruby C API is linked into the rlib).
    pub fn open() -> Result<Self, MrbOpenError> {
        #[cfg(mruby_linked)]
        {
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
        #[cfg(not(mruby_linked))]
        {
            Err(MrbOpenError)
        }
    }

    /// Raw `*mut mrb_state`. Use only at FFI boundaries that have
    /// not yet migrated to safe methods. The returned pointer is
    /// valid for the lifetime of `&self`; callers must not call
    /// `mrb_close` on it (the `Mrb` Drop owns that).
    #[inline]
    pub fn as_ptr(&self) -> *mut sys::mrb_state {
        #[cfg(mruby_linked)]
        {
            self.state.as_ptr()
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
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
        #[cfg(mruby_linked)]
        {
            debug_assert!(!mrb_ref.is_null());
            // SAFETY: `Mrb` is `#[repr(transparent)]` over
            // `NonNull<mrb_state>`, which is itself `#[repr(transparent)]`
            // over `*mut mrb_state`. So a `*const *mut mrb_state` (the
            // address of the caller's pointer variable) and a `*const Mrb`
            // index into the same storage layout. The borrow lifetime is
            // inherited from `mrb_ref` via lifetime elision.
            unsafe { &*(mrb_ref as *const *mut sys::mrb_state as *const Mrb) }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb_ref;
            crate::not_linked()
        }
    }

    /// Return the currently pending mruby exception, or
    /// `mrb_nil_value()` (`w == 0`) if none. Reads `mrb->exc`
    /// directly through the bindgen-exposed struct field; does NOT
    /// clear the field — callers pair this with `Mrb::clear_exc`
    /// after they have captured class/message/backtrace.
    pub fn pending_exc(&self) -> Value {
        #[cfg(mruby_linked)]
        {
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
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// Set `mrb->exc` to `exc`, replacing whatever was there. Used by
    /// synthesis paths where an FFI call signals failure by returning
    /// NULL without raising — see `Mrb::load_bytecode`'s
    /// `mrb_read_irep_buf` recovery. Most code paths should let mruby
    /// raise via `mrb_raise` from inside a C bridge instead; that path
    /// triggers the normal exception flow without needing a manual
    /// slot write.
    ///
    /// `exc` must be an exception-object-tagged `Value`; mruby's
    /// downstream machinery dereferences the slot as `RObject *`. Pass
    /// nil or a non-object value at your peril (segfault on the next
    /// exception check).
    pub fn set_pending_exc(&self, exc: Value) {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self.state` is alive by the `&self` borrow; `exc`
            // originates from the same VM. `mrb_obj_ptr_func` extracts the
            // RObject pointer carried by the value; the assignment installs
            // it as the new pending exception, replacing whatever sat in
            // the slot.
            let obj_ptr = unsafe { sys::mrb_obj_ptr_func(exc.into_raw()) };
            unsafe { (*self.state.as_ptr()).exc = obj_ptr };
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = exc;
            crate::not_linked()
        }
    }

    /// Clear `mrb->exc`. Idempotent; safe to call when no exception
    /// is pending. Used by the consumer crate's panic-recovery paths
    /// after the pending exception has been extracted, so subsequent
    /// mruby calls do not observe stale exception state.
    pub fn clear_exc(&self) {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self.state` is alive by the `&self` borrow. The
            // return value (a `mrb_bool` snapshot of the prior
            // `mrb->exc` state) is intentionally discarded.
            let _ = unsafe { sys::mrb_check_error(self.as_ptr()) };
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// Return `mrb->object_class` as a typed `RClass` handle.
    /// Replaces direct field access — the `object_class` field on
    /// the `crate::mrb_state` struct is `pub(crate)` so this
    /// accessor is the one external entry point. The free function
    /// `crate::mrb_object_class` remains for code paths that hold
    /// only a raw `*mut mrb_state`.
    #[inline]
    pub fn object_class(&self) -> RClass {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self.state` is alive by the `&self` borrow.
            RClass::from_raw(unsafe { sys::mrb_object_class(self.as_ptr()) })
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }
}

#[cfg(mruby_linked)]
impl Drop for Mrb {
    fn drop(&mut self) {
        // SAFETY: `state` was produced by `mrb_open` in `Mrb::open`
        // and has not been closed elsewhere — `as_ptr` hands out
        // borrows but never takes ownership.
        unsafe { sys::mrb_close(self.state.as_ptr()) };
    }
}

#[cfg(not(mruby_linked))]
impl Drop for Mrb {
    fn drop(&mut self) {
        // Unreachable: `Mrb::open` always returns `Err` in
        // placeholder builds, so no `Mrb` value can be constructed.
        // Required only so the type satisfies `Drop` uniformly
        // across builds.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(mruby_linked)]
    #[test]
    fn open_boots_and_closes_a_live_interpreter() {
        // With a vendored libmruby.a linked, `open` boots a real
        // interpreter through `mrb_open`; the drop runs `mrb_close`.
        // This is the host-native smoke test of the whole link graph
        // (bindings + trampolines + libmruby.a).
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        drop(mrb);
    }

    #[cfg(not(mruby_linked))]
    #[test]
    fn open_returns_error_without_mruby() {
        // Placeholder mode: `mrb_open` is not linked, so `open` must
        // yield `Err` without attempting an FFI call. This is the
        // documented contract for builds without a staged libmruby.a.
        assert_eq!(
            Mrb::open().err(),
            Some(MrbOpenError),
            "Mrb::open without libmruby.a must return Err(MrbOpenError) without invoking FFI"
        );
    }
}
