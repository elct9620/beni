//! What the typed surface keeps per interpreter in mruby's
//! auxiliary-data field (`mrb->ud`).
//!
//! mruby publishes no call for the field, so the typed surface owns it:
//! it is NULL or points at the `Slot` an owner allocated, which lives
//! until the owner drops the interpreter. A borrow reaches the slot in
//! place and an owner rewrites it; a state beni did not open carries
//! none. A write to the field through `beni::sys` is the writer's own
//! unsafe act.

use crate::Mrb;
use core::any::Any;

/// Everything one interpreter keeps for the typed surface.
#[derive(Default)]
pub(crate) struct Slot {
    /// The embedder's user data.
    pub(crate) user: Option<Box<dyn Any + Send>>,
    /// The installed code fetch hook. Its trampoline reaches it through
    /// a borrow of the interpreter while the owner's borrows may read
    /// the user data, so it sits in its own cell.
    #[cfg(mrb_debug_hook)]
    pub(crate) hook: core::cell::UnsafeCell<Option<crate::debug::Hook>>,
    /// The lazy-value record, once a read has found it under its global,
    /// which keeps it reachable.
    pub(crate) lazy_record: core::cell::Cell<Option<crate::RArray>>,
}

impl Mrb {
    /// The slot this interpreter carries, or nothing for a state beni
    /// did not open.
    pub(crate) fn slot(&self) -> Option<&Slot> {
        // SAFETY: `self` is live, and `ud` is NULL or the pointer
        // `install_slot` wrote, which stays valid until `release_slot`
        // reclaims it through `&mut self` — so not while this borrow
        // lives.
        unsafe { ((*self.as_ptr()).ud as *const Slot).as_ref() }
    }

    /// Give this interpreter its slot. An owner does so once, as it opens
    /// the interpreter, and its drop reclaims the slot.
    pub(crate) fn install_slot(&mut self) {
        *self.ud_field_mut() = Box::into_raw(Box::<Slot>::default()).cast();
    }

    /// The slot an owner's interpreter carries.
    pub(crate) fn slot_mut(&mut self) -> &mut Slot {
        // SAFETY: an owner's `ud` is the pointer `install_slot` wrote,
        // valid until `release_slot` reclaims it after the drop, and
        // `&mut self` makes this the only borrow.
        unsafe { self.ud_field().cast::<Slot>().as_mut() }
            .expect("an interpreter beni opened carries its slot")
    }

    /// The field as it stands, read before the interpreter closes so
    /// `release_slot` can reclaim the slot afterwards.
    pub(crate) fn ud_field(&self) -> *mut core::ffi::c_void {
        // SAFETY: `self` is live.
        unsafe { (*self.as_ptr()).ud }
    }

    fn ud_field_mut(&mut self) -> &mut *mut core::ffi::c_void {
        // SAFETY: `self` is live and `&mut self` makes this the only
        // access to the field.
        unsafe { &mut (*self.as_ptr()).ud }
    }
}

/// Reclaim the slot `ud` points at, if any. Runs once the interpreter
/// has closed, so nothing mruby runs reads it after.
pub(crate) fn release_slot(ud: *mut core::ffi::c_void) {
    if !ud.is_null() {
        // SAFETY: a non-NULL `ud` is the pointer `install_slot` produced
        // from `Box::into_raw`, read out of the state before it closed,
        // so this is its only reclaim.
        drop(unsafe { Box::from_raw(ud.cast::<Slot>()) });
    }
}
