//! GC roots on `Mrb`.
//!
//! The arena reaches only as far as the C frame that opened it, so a
//! value a Rust caller holds past that frame needs a root — a hold that
//! keeps it reachable for the collector independently of the arena and
//! of any Ruby reference to it.
//!
//! `Mrb::gc_register_forever` is the shape whose root is never released.
//! Never releasing is what makes it safe: mruby's registry is keyed by
//! value rather than by registration, so a removal drops every root over
//! that value, and a shape that never removes cannot drop another
//! holder's.
//!
//! `Mrb::gc_root` is the releasable shape, and it needs the per-root
//! identity mruby's registry lacks. Each root is a slot in an array the
//! interpreter holds, so releasing one clears that slot and leaves every
//! other slot — including another root over the same value — standing.

#[cfg(mruby_linked)]
use crate::{Array, FromValue as _};
use crate::{Error, Mrb, Value};
#[cfg(mruby_linked)]
use beni_sys as sys;

impl Mrb {
    /// Root `v` for this interpreter's remaining lifetime, so it stays
    /// reachable for the collector however the arena moves and whether
    /// or not Ruby references it. The root is never released, so the
    /// value is never reclaimed — the shape for a value an embedder
    /// holds as long as the VM itself, such as a cached class handle.
    /// Rooting an immediate value is a no-op, immediates being values
    /// the collector never reclaims.
    pub fn gc_register_forever(&self, v: Value) {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is alive by the `&self` borrow and `v`
            // originates from the same VM; the registry only records the
            // value, and this shape never removes it again.
            unsafe { sys::mrb_gc_register(self.as_ptr(), v.as_raw()) };
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = v;
            crate::not_linked()
        }
    }

    /// Root `v` until the returned guard is dropped, so it stays
    /// reachable for the collector however the arena moves and whether
    /// or not Ruby references it. Each guard owns one root: dropping it
    /// releases that root alone, leaving any other root over the same
    /// value standing. Fallible because taking a root grows the record
    /// of roots — no root is taken when it fails.
    pub fn gc_root(&self, v: Value) -> Result<GcRoot<'_>, Error> {
        #[cfg(mruby_linked)]
        {
            let table = self.root_table()?;
            let slot = table.claim(self, v)?;
            Ok(GcRoot {
                mrb: self,
                table,
                slot,
                value: v,
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = v;
            crate::not_linked()
        }
    }

    /// The array holding one slot per live root, created on first use
    /// and kept reachable for the interpreter's lifetime by the global
    /// it is stored under. The name carries no `$`, so no Ruby program
    /// can reach the table by writing a global variable.
    #[cfg(mruby_linked)]
    fn root_table(&self) -> Result<RootTable, Error> {
        let slot = self.intern_static(TABLE_GLOBAL);
        if let Some(table) = Array::from_value(self.gv_get(slot)) {
            return Ok(RootTable(table));
        }

        let table = self.ary_new();
        // Index 0 is the free-list head rather than a root, so a slot
        // index is never zero and zero can mean "no free slot".
        table.push(self, Value::from_int(self, 0))?;
        self.gv_set(slot, table.as_value());
        Ok(RootTable(table))
    }
}

/// The root table: an intrusive free list over an mruby array. Slot 0
/// holds the index of the first free slot (zero when there is none),
/// and each free slot holds the index of the next one, so a released
/// slot is reused without scanning.
#[cfg(mruby_linked)]
#[derive(Clone, Copy)]
struct RootTable(Array);

#[cfg(mruby_linked)]
impl RootTable {
    /// Store `v` in a free slot, reusing a released one when the free
    /// list has any and growing the table when it does not.
    fn claim(&self, mrb: &Mrb, v: Value) -> Result<usize, Error> {
        match self.free_head(mrb) {
            0 => {
                self.0.push(mrb, v)?;
                Ok(self.0.len() - 1)
            }
            head => {
                let next = self.0.entry(head as isize);
                self.0.store(mrb, 0, next)?;
                self.0.store(mrb, head as isize, v)?;
                Ok(head)
            }
        }
    }

    /// Clear `slot` and thread it onto the front of the free list.
    fn release(&self, mrb: &Mrb, slot: usize) -> Result<(), Error> {
        let head = self.0.entry(0);
        self.0.store(mrb, slot as isize, head)?;
        self.0
            .store(mrb, 0, Value::from_int(mrb, slot as sys::mrb_int))
    }

    /// The first free slot, or zero when there is none. Slot 0 always
    /// holds the integer this type wrote there, so a read that does not
    /// answer one means the table is unusable — reported as "no free
    /// slot", which grows the table instead of reusing a wrong one.
    fn free_head(&self, mrb: &Mrb) -> usize {
        self.0
            .entry(0)
            .as_int(mrb)
            .ok()
            .and_then(|head| usize::try_from(head).ok())
            .unwrap_or(0)
    }
}

/// The global the root table is stored under. The name carries no `$`,
/// so no Ruby program can reach the table by writing a global variable.
#[cfg(mruby_linked)]
const TABLE_GLOBAL: &[u8] = b"beni_gc_roots";

/// One root over one value, released when dropped. Roots over the same
/// value are independent: each owns its own slot, so no drop releases
/// another's. The guard holds the table and the value it rooted, so
/// reading it back and releasing it need no lookup.
///
/// The guard borrows the interpreter, so it stays on the thread that
/// made it:
///
/// ```compile_fail
/// fn carried<T: Send>() {}
/// carried::<beni::GcRoot<'static>>();
/// ```
pub struct GcRoot<'mrb> {
    #[cfg(mruby_linked)]
    mrb: &'mrb Mrb,
    #[cfg(mruby_linked)]
    table: RootTable,
    #[cfg(mruby_linked)]
    slot: usize,
    #[cfg(mruby_linked)]
    value: Value,
    #[cfg(not(mruby_linked))]
    _mrb: core::marker::PhantomData<&'mrb Mrb>,
}

impl GcRoot<'_> {
    /// The value this root holds. Its slot changes only when the root
    /// is released, which consumes the guard, so this stays what was
    /// rooted for as long as the guard lives.
    pub fn value(&self) -> Value {
        #[cfg(mruby_linked)]
        {
            self.value
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }
}

impl Drop for GcRoot<'_> {
    fn drop(&mut self) {
        #[cfg(mruby_linked)]
        {
            // A refused release leaves the value rooted for the
            // interpreter's remaining lifetime — over-retention, never a
            // value collected while a holder still names it.
            let _ = self.table.release(self.mrb, self.slot);
        }
    }
}

#[cfg(all(test, mruby_linked))]
mod tests {
    use super::*;
    use crate::{FromValue, RString};

    #[test]
    fn a_released_slot_is_reused_by_the_next_root() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let first = mrb.str_new(b"first").as_value();
        let second = mrb.str_new(b"second").as_value();

        let root = mrb.gc_root(first).expect("taking a root must succeed");
        let slot = root.slot;
        drop(root);

        let reused = mrb.gc_root(second).expect("taking a root must succeed");
        assert_eq!(
            reused.slot, slot,
            "a released slot must be reused rather than left to grow the table"
        );
        assert_eq!(
            RString::from_value(reused.value())
                .expect("the reused slot holds the second value")
                .to_bytes(),
            b"second"
        );
    }
}
