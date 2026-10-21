//! The one piece of user data an interpreter holds for its embedder,
//! kept in the interpreter's slot.

use crate::Mrb;

impl Mrb {
    /// Install `value` as this interpreter's user data. An interpreter
    /// that already holds user data refuses it, handing `value` back as
    /// the `Err` and leaving the held one in place; replacing is taking
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

    /// The user data in place, when it is a `T`. Answers nothing when
    /// the interpreter holds none or holds another type.
    pub fn user_data<T: 'static>(&self) -> Option<&T> {
        self.slot()?.user.as_ref()?.downcast_ref::<T>()
    }

    /// Take the user data out, leaving none, when it is a `T`. Answers
    /// nothing when the interpreter holds none or holds another type,
    /// and leaves what it holds in place.
    pub fn take_user_data<T: 'static>(&mut self) -> Option<T> {
        let user = &mut self.slot_mut().user;
        if !user.as_ref()?.is::<T>() {
            return None;
        }
        user.take()?.downcast::<T>().ok().map(|value| *value)
    }

    /// Drop the user data in place. Runs as the interpreter closes.
    pub(crate) fn drop_user_data(&mut self) {
        self.slot_mut().user = None;
    }
}
