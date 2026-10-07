//! `Lazy`: a value a `static` names, computed once in each interpreter.
//!
//! magnus's `Lazy` holds one value for the process and roots it forever.
//! mruby runs many interpreters in a process and releases a root with its
//! state, so the `static` here holds only an index, assigned once per
//! process, and each interpreter keeps the value at that index in its own
//! record — an Array under a global whose name carries no `$`, which no
//! Ruby program can read or write. An unheld slot holds undef, a value no
//! computed handle can be.

use crate::{sys::AsRawValue, FromValue as _, Mrb, RArray, ReprValue, Value};
use beni_sys as sys;
use core::sync::atomic::{AtomicU32, Ordering};

/// The global holding this interpreter's lazy values.
const RECORD_GLOBAL: &[u8] = b"beni_lazy";

/// The next index a `Lazy` takes, shared by every `Lazy` in the process.
static NEXT_INDEX: AtomicU32 = AtomicU32::new(0);

/// A value a `static` names, computed by `func` from the interpreter the
/// first time that interpreter asks for it and held for that
/// interpreter's lifetime. Mirrors magnus's `value::Lazy`, holding one
/// value in each interpreter rather than one for the process, since an
/// mruby process runs many interpreters and releases a root with its
/// state.
///
/// ```
/// use beni::value::Lazy;
/// use beni::{Mrb, RString};
///
/// static GREETING: Lazy<RString> = Lazy::new(|mrb| mrb.str_new(b"hello"));
///
/// let mrb = Mrb::open().unwrap();
/// assert_eq!(mrb.get_inner(&GREETING).to_string(&mrb).unwrap(), "hello");
/// ```
pub struct Lazy<T> {
    func: fn(&Mrb) -> T,
    /// The index plus one, or zero before the first use assigns it.
    index: AtomicU32,
}

impl<T: ReprValue> Lazy<T> {
    /// A `Lazy` computing its value with `func`. Mirrors magnus's
    /// `Lazy::new`.
    pub const fn new(func: fn(&Mrb) -> T) -> Self {
        Self {
            func,
            index: AtomicU32::new(0),
        }
    }

    /// Compute and hold the value in `mrb` now, when it holds none yet.
    /// Mirrors magnus's `Lazy::force`.
    pub fn force(this: &Self, mrb: &Mrb) {
        mrb.get_inner(this);
    }

    /// The value `mrb` holds, or `None` when it holds none yet; computes
    /// nothing. Mirrors magnus's `Lazy::try_get_inner`, reading the
    /// interpreter that holds the value.
    pub fn try_get_inner(this: &Self, mrb: &Mrb) -> Option<T> {
        let held = mrb.lazy_slot(this.index())?;
        // SAFETY: a held slot holds the value this `Lazy` computed or was
        // given, a `T`.
        Some(unsafe { <T as crate::value::private::ReprValue>::from_value_unchecked(held) })
    }

    /// Hold `value` in `mrb` in place of whatever it held.
    pub(crate) fn hold(this: &Self, mrb: &Mrb, value: T) {
        mrb.set_lazy_slot(this.index(), value.as_value());
    }

    fn index(&self) -> usize {
        let held = self.index.load(Ordering::Acquire);
        if held != 0 {
            return held as usize - 1;
        }
        let taken = NEXT_INDEX.fetch_add(1, Ordering::Relaxed) + 1;
        match self
            .index
            .compare_exchange(0, taken, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => taken as usize - 1,
            Err(raced) => raced as usize - 1,
        }
    }
}

impl Mrb {
    /// The value `lazy` names in this interpreter, computing and holding
    /// it first when this interpreter holds none. Mirrors magnus's
    /// `Ruby::get_inner`.
    pub fn get_inner<T: ReprValue>(&self, lazy: &Lazy<T>) -> T {
        if let Some(held) = Lazy::try_get_inner(lazy, self) {
            return held;
        }
        let value = (lazy.func)(self);
        Lazy::hold(lazy, self, value);
        value
    }

    fn lazy_slot(&self, index: usize) -> Option<Value> {
        let record = self.lazy_record()?;
        if index >= record.len() {
            return None;
        }
        // SAFETY: the record, held under its global for the interpreter's
        // lifetime, keeps every value it holds reachable.
        let held = unsafe { record.entry_unheld(index) };
        // SAFETY: pure tag check.
        (!unsafe { sys::mrb_undef_p_func(held.as_raw()) }).then_some(held)
    }

    fn set_lazy_slot(&self, index: usize, value: Value) {
        let record = self.lazy_record().unwrap_or_else(|| {
            let record = self.ary_new();
            let name = self
                .intern_static(RECORD_GLOBAL)
                .expect("interning a short static name only allocates");
            self.gv_set(name, record.as_value())
                .expect("a global no Ruby program can name is never frozen");
            record
        });
        // SAFETY: pure value computation.
        let unheld = Value::from_raw_unchecked(unsafe { sys::mrb_undef_value_func() });
        while record.len() < index {
            record
                .push(self, unheld)
                .expect("the record no Ruby program reaches is never frozen");
        }
        record
            .store(self, index as isize, value)
            .expect("the record no Ruby program reaches is never frozen");
    }

    fn lazy_record(&self) -> Option<RArray> {
        let name = self.check_id(RECORD_GLOBAL)?;
        // SAFETY: `self` is alive; reading a global neither raises nor
        // dispatches.
        let record = unsafe { sys::mrb_gv_get(self.as_ptr(), name.to_raw()) };
        RArray::from_value(Value::from_raw_unchecked(record))
    }
}
