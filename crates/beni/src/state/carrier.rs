//! The carrier record: the classes `TypedData` and `InlineStruct`
//! types were marked as in this interpreter.
//!
//! A type implemented by `#[beni::wrap]`, `#[derive(TypedData)]`, or
//! `#[derive(InlineStruct)]` names each class by path at a naming site —
//! the type's own `class` or an enum variant's — that holds a `Lazy`. The
//! path resolves once, when the embedder marks the type's carriers, and
//! every later naming reads the class the site's `Lazy` holds, so a
//! constant a Ruby program binds over that path reaches no wrap. The
//! classes inline-struct types own are kept beside it under a global
//! whose name carries no `$`, which no Ruby program can write.
//!
//! A read checks a symbol and reads a global, then indexes an Array or
//! scans class and type pairs compared by pointer: none allocates,
//! raises, or dispatches, so a read runs no Ruby a program could define
//! and needs no protect frame.

use crate::value::Lazy;
use crate::{
    sys::AsRawValue, Error, FromValue as _, Mrb, RArray, RClass, ReprValue, TryConvert, Value,
};
use beni_sys as sys;
use core::ffi::CStr;

/// The global holding the classes inline-struct types own.
const INLINE_GLOBAL: &[u8] = b"beni_inline";

impl Mrb {
    /// Resolve `path` from `Object`, prepare the class it names to carry
    /// Rust data — marked as a carrier, its default allocator undefined —
    /// and hold it as `site`'s class in this interpreter, in place of any
    /// class `site` held. The `TypedData` macros mark each class they name
    /// through it.
    ///
    /// Each segment of `path` is fetched as a constant of the segment
    /// before it, the first as a constant of `Object`, so
    /// `c"Outer::Inner"` names a nested class. Surfaces an `Err` when a
    /// segment resolves to no constant, when the path resolves to a
    /// value that is not a class, or when that class refuses the
    /// carrier mark.
    #[doc(hidden)]
    pub fn mark_carrier_site(
        &self,
        site: &Lazy<RClass>,
        path: &'static CStr,
    ) -> Result<RClass, Error> {
        let class = self.prepare_carrier(path, |class| class.set_instance_data_tt(self))?;
        Lazy::hold(site, self, class);
        Ok(class)
    }

    /// As `mark_carrier_site`, preparing the class `path` names so its
    /// instances are inline structs of `T` rather than data carriers. The
    /// `InlineStruct` macros mark the class they name through it.
    #[doc(hidden)]
    pub fn mark_inline_carrier_site<T: crate::InlineStruct>(
        &self,
        site: &Lazy<RClass>,
        path: &'static CStr,
    ) -> Result<RClass, Error> {
        let class = self.prepare_carrier(path, |class| class.set_instance_inline_tt::<T>(self))?;
        Lazy::hold(site, self, class);
        Ok(class)
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
        Ok(class)
    }

    /// The whole path as the symbol keying it in the record. A symbol
    /// key is hashed and compared by its id, so reading the record
    /// runs no Ruby a program could define.
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
