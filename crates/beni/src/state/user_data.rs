//! The one piece of user data an interpreter holds for its embedder,
//! kept in the interpreter's slot.

use crate::Mrb;

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
        let user = &mut self.slot_mut().user;
        if user.is_some() {
            return Err(value);
        }
        *user = Some(Box::new(value));
        Ok(())
    }

    /// The user data in place, when the slot holds a `T`. Answers
    /// nothing for an empty slot or one holding another type.
    pub fn user_data<T: 'static>(&self) -> Option<&T> {
        self.slot()?.user.as_ref()?.downcast_ref::<T>()
    }

    /// Take the user data out, leaving the slot empty, when the slot
    /// holds a `T`. Answers nothing for an empty slot or one holding
    /// another type, and leaves such a slot as it was.
    pub fn take_user_data<T: 'static>(&mut self) -> Option<T> {
        let user = &mut self.slot_mut().user;
        if !user.as_ref()?.is::<T>() {
            return None;
        }
        user.take()?.downcast::<T>().ok().map(|value| *value)
    }

    /// Drop the user data in place. Runs as the interpreter closes.
    pub(crate) fn drop_user_data(&mut self) {
        if self.slot().is_some() {
            self.slot_mut().user = None;
        }
    }
}
