//! The one slot of user data an interpreter holds for its embedder.
//!
//! mruby's state carries an auxiliary-data field (`mrb->ud`) and
//! publishes no call for it, so the typed surface owns the field: it
//! points at the boxed value installed here, or is NULL. A write to it
//! through `beni::sys` is the writer's own unsafe act.

use crate::Mrb;
use core::any::Any;

/// What `mrb->ud` points at while the slot holds a value. The trait
/// object is boxed again so the field, a thin pointer, can carry it.
type Slot = Box<dyn Any + Send>;

impl Mrb {
    /// Install `value` as this interpreter's user data. A slot that
    /// already holds a value refuses it, handing `value` back as the
    /// `Err` and leaving the held one in place; replacing is taking
    /// first.
    ///
    /// Only the owner installs — a gem's installation and a registered
    /// method hold a borrow, so they read and never write:
    ///
    /// ```compile_fail
    /// fn install_through_a_borrow(mrb: &beni::Mrb) {
    ///     let _ = mrb.set_user_data(1u8);
    /// }
    /// ```
    ///
    /// The value crosses threads with the interpreter carrying it:
    ///
    /// ```compile_fail
    /// fn install_unsendable(mrb: &mut beni::Mrb) {
    ///     let _ = mrb.set_user_data(std::rc::Rc::new(1u8));
    /// }
    /// ```
    pub fn set_user_data<T: Send + 'static>(&mut self, value: T) -> Result<(), T> {
        if self.slot().is_some() {
            return Err(value);
        }
        self.store(Box::new(value));
        Ok(())
    }

    /// The user data in place, when the slot holds a `T`. Answers
    /// nothing for an empty slot or one holding another type.
    pub fn user_data<T: 'static>(&self) -> Option<&T> {
        self.slot()?.downcast_ref::<T>()
    }

    /// Take the user data out, leaving the slot empty, when the slot
    /// holds a `T`. Answers nothing for an empty slot or one holding
    /// another type, and leaves such a slot as it was.
    pub fn take_user_data<T: 'static>(&mut self) -> Option<T> {
        match self.release()?.downcast::<T>() {
            Ok(value) => Some(*value),
            Err(other) => {
                self.store(other);
                None
            }
        }
    }

    /// Drop whatever the slot holds. Runs as the interpreter closes.
    pub(crate) fn drop_user_data(&mut self) {
        drop(self.release());
    }

    fn slot(&self) -> Option<&Slot> {
        // SAFETY: `self` is live, and `ud` is NULL or the pointer
        // `store` wrote, which stays valid until `release` takes it
        // back through `&mut self` — so not while this borrow lives.
        unsafe { ((*self.as_ptr()).ud as *const Slot).as_ref() }
    }

    fn store(&mut self, slot: Slot) {
        // SAFETY: `self` is live; the slot is empty, so no earlier
        // value is overwritten.
        unsafe { (*self.as_ptr()).ud = Box::into_raw(Box::new(slot)).cast() };
    }

    fn release(&mut self) -> Option<Slot> {
        // SAFETY: `self` is live; the field is read and cleared in one
        // step, so the pointer is reclaimed exactly once.
        let ud = unsafe { core::mem::replace(&mut (*self.as_ptr()).ud, core::ptr::null_mut()) };
        // SAFETY: a non-NULL `ud` is the pointer `store` produced from
        // `Box::into_raw`, and clearing the field above makes this its
        // only reclaim.
        (!ud.is_null()).then(|| *unsafe { Box::from_raw(ud.cast::<Slot>()) })
    }
}
