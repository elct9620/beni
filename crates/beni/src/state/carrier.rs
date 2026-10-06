//! The carrier record: the classes `TypedData` and `InlineStruct`
//! types were marked as in this interpreter.
//!
//! A type implemented by `#[beni::wrap]` or `#[derive(TypedData)]`
//! names its class by path. The path resolves once, when the embedder
//! marks the type's carriers, and every later naming reads the class
//! out of the record — so a constant a Ruby program binds over that
//! path reaches no wrap. The record — the class each path names, the
//! classes inline-struct types own, and each inline-struct type's own
//! class — is stored under globals whose names carry no `$`, which no
//! Ruby program can write.
//!
//! A read is a symbol check and a global read, then a hash fetch keyed
//! by symbol or a scan of class and type pairs compared by pointer:
//! none allocates, raises, or dispatches, so a read runs no Ruby a
//! program could define and needs no protect frame.

use crate::{
    sys::AsRawValue, Error, FromValue as _, Mrb, RArray, RClass, RHash, ReprValue, Symbol,
    TryConvert, Value,
};
use beni_sys as sys;
use core::ffi::CStr;

/// The global holding the class each path names.
const RECORD_GLOBAL: &[u8] = b"beni_carriers";

/// The global holding the classes inline-struct types own.
const INLINE_GLOBAL: &[u8] = b"beni_inline";

/// The global holding the class each inline-struct type was last
/// prepared as.
const INLINE_CLASS_GLOBAL: &[u8] = b"beni_inline_classes";

impl Mrb {
    /// Resolve `path` from `Object`, prepare the class it names to
    /// carry Rust data — marked as a carrier, its default allocator
    /// undefined — and hold it in this interpreter's carrier record
    /// under `path`. The class the record already held for `path`, if
    /// any, is replaced.
    ///
    /// Each segment of `path` is fetched as a constant of the segment
    /// before it, the first as a constant of `Object`, so
    /// `c"Outer::Inner"` names a nested class. Surfaces an `Err` when a
    /// segment resolves to no constant, when the path resolves to a
    /// value that is not a class, or when that class refuses the
    /// carrier mark.
    pub fn mark_carrier(&self, path: &'static CStr) -> Result<RClass, Error> {
        self.prepare_carrier(path, |class| class.set_instance_data_tt(self))
    }

    /// As `mark_carrier`, preparing the class `path` names so its
    /// instances are inline structs of `T` rather than data carriers,
    /// and holding it as `T`'s class in place of the one an earlier call
    /// for `T` held.
    pub fn mark_inline_carrier<T: crate::InlineStruct>(
        &self,
        path: &'static CStr,
    ) -> Result<RClass, Error> {
        let class = self.prepare_carrier(path, |class| class.set_instance_inline_tt::<T>(self))?;
        let classes = self.held_pairs(INLINE_CLASS_GLOBAL)?;
        let tag = T::inline_type().tag();
        match pairs(classes).position(|(_, held)| held == tag) {
            Some(pair) => classes.store(self, (pair * 2) as isize, class.as_value())?,
            None => self.push_pair(classes, class, tag)?,
        }
        Ok(class)
    }

    /// The class `mark_inline_carrier` last held as `T`'s in this
    /// interpreter, and nothing when it has held none — read by pointer
    /// identity alone, so the class belongs to `T` without a walk of its
    /// ancestry.
    pub fn inline_carrier<T: crate::InlineStruct>(&self) -> Option<RClass> {
        let classes = self.held_array(INLINE_CLASS_GLOBAL)?;
        let tag = T::inline_type().tag();
        let (class, _) = pairs(classes).find(|(_, held)| *held == tag)?;
        RClass::from_value(class)
    }

    /// The class this interpreter's carrier record holds for `path`,
    /// and nothing when `mark_carrier` has put none there.
    pub fn carrier(&self, path: &'static CStr) -> Option<RClass> {
        let record = self.held_record()?;
        let key = self.check_symbol(path.to_bytes())?.as_value();
        // SAFETY: `record` is the live record Hash; a symbol key is hashed
        // and compared by its id, and `mrb_hash_fetch` answers the given
        // default for an absent key without consulting the Hash's own, so
        // the fetch neither raises nor dispatches.
        let held = unsafe {
            sys::mrb_hash_fetch(
                self.as_ptr(),
                record.as_raw(),
                key.as_raw(),
                crate::value::qnil().as_value().as_raw(),
            )
        };
        RClass::from_value(Value::from_raw_unchecked(held))
    }

    /// Hold `class` in the record as belonging to the inline-struct type
    /// whose descriptor sits at `tag`, unless it already does.
    pub(crate) fn hold_inline(&self, class: RClass, tag: *const ()) -> Result<(), Error> {
        if self.inline_owner(class) == Some(tag) {
            return Ok(());
        }
        self.push_pair(self.held_pairs(INLINE_GLOBAL)?, class, tag)
    }

    /// Append the pair `class, tag` to `pairs`.
    fn push_pair(&self, pairs: RArray, class: RClass, tag: *const ()) -> Result<(), Error> {
        pairs.push(self, class.as_value())?;
        // SAFETY: `mrb_cptr_value` only boxes the address; the tag is a
        // `'static` descriptor's, so it outlives the interpreter.
        let boxed = unsafe { sys::mrb_cptr_value(self.as_ptr(), tag.cast_mut().cast()) };
        pairs.push(self, Value::from_raw_unchecked(boxed))
    }

    /// The tag of the inline-struct type `class` belongs to: the one
    /// held for the nearest class in its ancestry the record holds, read
    /// by pointer identity alone, so no Ruby a program defines runs.
    pub(crate) fn inline_owner(&self, class: RClass) -> Option<*const ()> {
        let owners = self.held_array(INLINE_GLOBAL)?;
        let mut current = class.as_internal();
        while !current.is_null() {
            // SAFETY: `current` walks the live superclass chain of a
            // class from this interpreter.
            let raw = unsafe { &*current };
            if raw.tt() != sys::MRB_TT_ICLASS {
                if let Some(tag) = owner_of(owners, current) {
                    return Some(tag);
                }
            }
            current = raw.super_;
        }
        None
    }

    /// The pairs `[class, tag, …]` stored under the global `name`,
    /// created on first use and kept reachable — with every class they
    /// hold — by that global.
    fn held_pairs(&self, name: &'static [u8]) -> Result<RArray, Error> {
        if let Some(pairs) = self.held_array(name) {
            return Ok(pairs);
        }
        let pairs = self.ary_new();
        self.gv_set(self.intern_static(name)?, pairs.as_value())?;
        Ok(pairs)
    }

    /// The record, created on first use and kept reachable for the
    /// interpreter's lifetime by the global it is stored under — which
    /// is also what keeps every class it holds reachable.
    fn carrier_record(&self) -> Result<RHash, Error> {
        if let Some(record) = self.held_record() {
            return Ok(record);
        }
        let record = self.hash_new();
        self.gv_set(self.intern_static(RECORD_GLOBAL)?, record.as_value())?;
        Ok(record)
    }

    /// The record, and nothing before anything has been marked.
    fn held_record(&self) -> Option<RHash> {
        RHash::from_value(self.held_global(RECORD_GLOBAL)?)
    }

    /// The array stored under the global `name`, and nothing before one
    /// was.
    fn held_array(&self, name: &'static [u8]) -> Option<RArray> {
        RArray::from_value(self.held_global(name)?)
    }

    /// The value of the global `name`, and nothing before it was set:
    /// a name never interned names no global. The global keeps the value
    /// reachable, so the read takes no arena slot.
    fn held_global(&self, name: &'static [u8]) -> Option<Value> {
        let name = self.check_id(name)?;
        // SAFETY: `self` is alive and `name` was interned against it.
        Some(Value::from_raw_unchecked(unsafe {
            sys::mrb_gv_get(self.as_ptr(), name.to_raw())
        }))
    }

    /// Resolve `path`, mark the class it names with `mark`, undefine its
    /// default allocator, and hold it in the record under `path`.
    fn prepare_carrier(
        &self,
        path: &'static CStr,
        mark: impl FnOnce(RClass) -> Result<(), Error>,
    ) -> Result<RClass, Error> {
        let class = self.resolve_carrier(path)?;
        mark(class)?;
        class.undef_default_alloc_func(self);
        let record = self.carrier_record()?;
        record.set(self, self.carrier_key(path)?, class.as_value())?;
        Ok(class)
    }

    /// The whole path as the symbol keying it in the record. A symbol
    /// key is hashed and compared by its id, so reading the record
    /// runs no Ruby a program could define.
    fn carrier_key(&self, path: &'static CStr) -> Result<Value, Error> {
        Ok(Symbol::from(self.intern_static(path.to_bytes())?).as_value())
    }

    /// Walk `path` from `Object`, one constant fetch per segment.
    fn resolve_carrier(&self, path: &'static CStr) -> Result<RClass, Error> {
        let mut named = self.object_class().as_value();
        for segment in segments(path.to_bytes()) {
            named = named.fetch_const(self, self.intern_static(segment)?)?;
        }
        RClass::try_convert(named, self)
    }
}

/// The tag `owners` holds for `class`, compared by class pointer.
fn owner_of(owners: RArray, class: *mut sys::RClass) -> Option<*const ()> {
    pairs(owners).find_map(|(held, tag)| {
        // SAFETY: the record holds a class at every even index.
        let held = unsafe { sys::mrb_obj_ptr_func(held.as_raw()) } as *mut sys::RClass;
        (held == class).then_some(tag)
    })
}

/// Each `class, tag` pair a record array holds, the class as its value
/// and the tag unboxed. The record keeps every entry reachable, so the
/// reads take no arena slot.
fn pairs(array: RArray) -> impl Iterator<Item = (Value, *const ())> {
    let entry = move |index: usize| {
        // SAFETY: `array` is the record's live array and `index` lies
        // inside it.
        unsafe { sys::mrb_ary_entry(array.as_raw(), index as sys::mrb_int) }
    };
    (0..array.len() / 2).map(move |pair| {
        // SAFETY: the record pairs every class with the C pointer
        // `push_pair` boxed for it.
        let tag = unsafe { sys::mrb_cptr_func(entry(pair * 2 + 1)) } as *const ();
        (Value::from_raw_unchecked(entry(pair * 2)), tag)
    })
}

/// The `::`-separated segments of a constant path. A path holding an
/// empty segment yields it, and the fetch of a name nothing is bound
/// under reports it.
fn segments(path: &'static [u8]) -> impl Iterator<Item = &'static [u8]> {
    let mut rest = Some(path);
    core::iter::from_fn(move || {
        let current = rest?;
        match current.windows(2).position(|pair| pair == b"::") {
            Some(at) => {
                rest = Some(&current[at + 2..]);
                Some(&current[..at])
            }
            None => {
                rest = None;
                Some(current)
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::segments;

    #[test]
    fn a_path_splits_at_each_separator() {
        let split = |path| segments(path).collect::<Vec<_>>();

        assert_eq!(split(b"Point"), [b"Point".as_slice()]);
        assert_eq!(
            split(b"Outer::Inner"),
            [b"Outer".as_slice(), b"Inner".as_slice()]
        );
        assert_eq!(
            split(b"Outer::"),
            [b"Outer".as_slice(), b"".as_slice()],
            "a trailing separator leaves a segment no constant is bound under"
        );
    }
}
