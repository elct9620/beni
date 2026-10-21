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
}

impl Mrb {
    /// The slot this interpreter carries, or nothing for a state beni
    /// did not open.
    pub(crate) fn slot(&self) -> Option<&Slot> {
        // SAFETY: `self` is live, and `ud` is NULL or the pointer
        // `slot_mut` wrote, which stays valid until `release_slot`
        // reclaims it through `&mut self` — so not while this borrow
        // lives.
        unsafe { ((*self.as_ptr()).ud as *const Slot).as_ref() }
    }

    /// The slot this interpreter carries, allocated first when it has
    /// none. Only an owner reaches it, and the owner's drop reclaims it.
    pub(crate) fn slot_mut(&mut self) -> &mut Slot {
        let ud = self.ud_field();
        if ud.is_null() {
            *self.ud_field_mut() = Box::into_raw(Box::<Slot>::default()).cast();
        }
        // SAFETY: the field now holds the pointer written above or by
        // an earlier call, and `&mut self` makes this the only borrow.
        unsafe { &mut *self.ud_field().cast::<Slot>() }
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
        // SAFETY: a non-NULL `ud` is the pointer `slot_mut` produced
        // from `Box::into_raw`, read out of the state before it closed,
        // so this is its only reclaim.
        drop(unsafe { Box::from_raw(ud.cast::<Slot>()) });
    }
}
