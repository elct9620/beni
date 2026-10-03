//! The carrier record: the class each `TypedData` class path was
//! marked as in this interpreter.
//!
//! A type implemented by `#[beni::wrap]` or `#[derive(TypedData)]`
//! names its class by path. The path resolves once, when the embedder
//! marks the type's carriers, and every later naming reads the class
//! out of the record — so a constant a Ruby program binds over that
//! path reaches no wrap. The record is stored under a global whose
//! name carries no `$`, which no Ruby program can write.

use crate::{
    sys::AsRawValue, Error, FromValue as _, Mrb, RArray, RClass, RHash, ReprValue, Symbol,
    TryConvert, Value,
};
use beni_sys as sys;
use core::ffi::CStr;

/// The global holding this interpreter's carrier record.
const RECORD_GLOBAL: &[u8] = b"beni_carriers";

/// The record entry holding the classes inline-struct types own.
const INLINE_KEY: &[u8] = b"beni_inline";

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
    /// instances are inline structs of `T` rather than data carriers.
    pub fn mark_inline_carrier<T: crate::InlineStruct>(
        &self,
        path: &'static CStr,
    ) -> Result<RClass, Error> {
        self.prepare_carrier(path, |class| class.set_instance_inline_tt::<T>(self))
    }

    /// The class this interpreter's carrier record holds for `path`,
    /// and nothing when `mark_carrier` has put none there.
    pub fn carrier(&self, path: &'static CStr) -> Option<RClass> {
        let held = self
            .held_record()?
            .get(self, self.carrier_key(path).ok()?)
            .ok()?;
        RClass::try_convert(held, self).ok()
    }

    /// Hold `class` in the record as belonging to the inline-struct type
    /// whose descriptor sits at `tag`, unless it already does.
    pub(crate) fn hold_inline(&self, class: RClass, tag: *const ()) -> Result<(), Error> {
        if self.inline_owner(class) == Some(tag) {
            return Ok(());
        }
        let owners = self.inline_owners()?;
        owners.push(self, class.as_value())?;
        // SAFETY: `mrb_cptr_value` only boxes the address; the tag is a
        // `'static` descriptor's, so it outlives the interpreter.
        let boxed = unsafe { sys::mrb_cptr_value(self.as_ptr(), tag.cast_mut().cast()) };
        owners.push(self, Value::from_raw_unchecked(boxed))
    }

    /// The tag of the inline-struct type `class` belongs to: the one
    /// held for the nearest class in its ancestry the record holds, read
    /// by pointer identity alone, so no Ruby a program defines runs.
    pub(crate) fn inline_owner(&self, class: RClass) -> Option<*const ()> {
        let owners = self.held_inline_owners()?;
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

    /// The pairs `[class, tag, …]` the record holds for inline-struct
    /// types, created on first use.
    fn inline_owners(&self) -> Result<RArray, Error> {
        if let Some(owners) = self.held_inline_owners() {
            return Ok(owners);
        }
        let owners = self.ary_new();
        self.carrier_record()?
            .set(self, self.inline_key()?, owners.as_value())?;
        Ok(owners)
    }

    fn held_inline_owners(&self) -> Option<RArray> {
        let held = self
            .held_record()?
            .get(self, self.inline_key().ok()?)
            .ok()?;
        RArray::from_value(held)
    }

    /// The key the pairs sit under: a name no constant path can spell,
    /// so no class path's entry collides with it.
    fn inline_key(&self) -> Result<Value, Error> {
        Ok(Symbol::from(self.intern_static(INLINE_KEY)?).as_value())
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
        let name = self.intern_static(RECORD_GLOBAL).ok()?;
        RHash::from_value(self.gv_get(name))
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
            named = named.const_get(self, self.intern_static(segment)?)?;
        }
        RClass::try_convert(named, self)
    }
}

/// The tag `owners` holds for `class`, compared by class pointer. The
/// record keeps every entry reachable, so the reads take no arena slot.
fn owner_of(owners: RArray, class: *mut sys::RClass) -> Option<*const ()> {
    let entry = |index: usize| {
        // SAFETY: `owners` is the record's live array and `index` lies
        // inside it.
        unsafe { sys::mrb_ary_entry(owners.as_raw(), index as sys::mrb_int) }
    };
    (0..owners.len() / 2).find_map(|pair| {
        // SAFETY: the record holds a class at every even index.
        let held = unsafe { sys::mrb_obj_ptr_func(entry(pair * 2)) } as *mut sys::RClass;
        // SAFETY: the record pairs every class with the C pointer
        // `hold_inline` boxed for it.
        (held == class).then(|| unsafe { sys::mrb_cptr_func(entry(pair * 2 + 1)) } as *const ())
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
