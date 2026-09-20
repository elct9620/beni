//! The carrier record: the class each `TypedData` class path was
//! marked as in this interpreter.
//!
//! A type implemented by `#[beni::wrap]` or `#[derive(TypedData)]`
//! names its class by path. The path resolves once, when the embedder
//! marks the type's carriers, and every later naming reads the class
//! out of the record — so a constant a Ruby program binds over that
//! path reaches no wrap. The record is stored under a global whose
//! name carries no `$`, which no Ruby program can write.

use crate::{Error, FromValue as _, Hash, Mrb, RClass, ReprValue, Symbol, TryConvert, Value};
use core::ffi::CStr;

/// The global holding this interpreter's carrier record.
const RECORD_GLOBAL: &[u8] = b"beni_carriers";

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
        let class = self.resolve_carrier(path)?;
        class.set_instance_data_tt(self)?;
        class.undef_default_alloc_func(self);
        let record = self.carrier_record()?;
        record.set(self, self.carrier_key(path)?, class.as_value())?;
        Ok(class)
    }

    /// The class this interpreter's carrier record holds for `path`,
    /// and nothing when `mark_carrier` has put none there.
    pub fn carrier(&self, path: &'static CStr) -> Option<RClass> {
        let name = self.intern_static(RECORD_GLOBAL).ok()?;
        let record = Hash::from_value(self.gv_get(name))?;
        let held = record.get(self, self.carrier_key(path).ok()?).ok()?;
        RClass::try_convert(held, self).ok()
    }

    /// The record, created on first use and kept reachable for the
    /// interpreter's lifetime by the global it is stored under — which
    /// is also what keeps every class it holds reachable.
    fn carrier_record(&self) -> Result<Hash, Error> {
        let name = self.intern_static(RECORD_GLOBAL)?;
        if let Some(record) = Hash::from_value(self.gv_get(name)) {
            return Ok(record);
        }
        let record = self.hash_new();
        self.gv_set(name, record.as_value())?;
        Ok(record)
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
