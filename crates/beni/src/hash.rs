//! Typed `Hash` newtype around a Hash-tagged `Value`.
//!
//! `Hash` is `#[repr(transparent)]` over `Value` (which is itself
//! `#[repr(transparent)]` over `mrb_value`). The two share their
//! in-memory layout — `Hash` is exactly an `mrb_value` known to carry
//! an mruby `Hash`. Construction is by explicit unchecked cast from
//! `Value`; element operations cluster on the resulting newtype.
//!
//! Mirrors magnus's `src/r_hash.rs`: factories live on `Ruby` /
//! `Mrb`, per-hash ops (`set`, `get`, `keys`) live here.

use crate::{Array, Mrb, Value};
use beni_sys as sys;

/// Typed handle on an mruby `Hash`. `#[repr(transparent)]` over
/// `Value` so the C ABI is preserved.
///
/// Construct via `Mrb::hash_new` (fresh hash), the checked
/// `FromValue` downcast (`Hash::from_value`, tag-discriminated), or
/// `Hash::from_value_unchecked` (assert that a `Value` you
/// already hold is Hash-tagged). Round-trip back to a generic
/// `Value` via `Hash::as_value` for APIs that take any value.
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct Hash(Value);

impl Hash {
    /// Wrap a `Value` that the caller has already determined to be
    /// Hash-tagged (e.g. via a `classname` check or because it came
    /// straight from `mrb_hash_new` / a host hash decoder).
    ///
    /// # Safety
    ///
    /// `v` must be Hash-tagged. Operating on a non-Hash value
    /// through this newtype is undefined per mruby's macro contract.
    #[inline]
    pub unsafe fn from_value_unchecked(v: Value) -> Self {
        Self(v)
    }

    /// Reify as a generic `Value` for APIs that accept any value.
    #[inline]
    pub fn as_value(self) -> Value {
        self.0
    }

    /// Borrow the inner `mrb_value` for raw FFI calls that have not
    /// yet migrated.
    #[inline]
    pub fn as_raw(self) -> sys::mrb_value {
        self.0.as_raw()
    }

    /// `mrb_hash_set(mrb, self, key, val)` — assign `key => val`.
    #[inline]
    pub fn set(self, mrb: &Mrb, key: Value, val: Value) {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `mrb` is alive; `self` is Hash-tagged by the
            // `from_value_unchecked` contract; `key` and `val` originate
            // from the same VM.
            unsafe { sys::mrb_hash_set(mrb.as_ptr(), self.0.as_raw(), key.as_raw(), val.as_raw()) };
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, key, val);
            crate::not_linked()
        }
    }

    /// `mrb_hash_get(mrb, self, key)` — return the value for `key`,
    /// or `nil` when absent.
    #[inline]
    pub fn get(self, mrb: &Mrb, key: Value) -> Value {
        #[cfg(mruby_linked)]
        {
            // SAFETY: as `set`.
            Value::from_raw(unsafe {
                sys::mrb_hash_get(mrb.as_ptr(), self.0.as_raw(), key.as_raw())
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, key);
            crate::not_linked()
        }
    }

    /// `mrb_hash_keys(mrb, self)` — return the Array of keys as a
    /// typed `Array`.
    #[inline]
    pub fn keys(self, mrb: &Mrb) -> Array {
        #[cfg(mruby_linked)]
        {
            // SAFETY: as `set`; `mrb_hash_keys` always returns an
            // Array-tagged value, so the unchecked wrap is sound.
            unsafe {
                Array::from_value_unchecked(Value::from_raw(sys::mrb_hash_keys(
                    mrb.as_ptr(),
                    self.0.as_raw(),
                )))
            }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }
}

#[cfg(all(test, mruby_linked))]
mod tests {
    use crate::Mrb;

    #[test]
    fn set_and_get_roundtrip_with_nil_for_an_absent_key() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let hash = mrb.hash_new();

        hash.set(
            &mrb,
            mrb.str_new(b"k").as_value(),
            mrb.str_new(b"v").as_value(),
        );

        assert_eq!(
            hash.get(&mrb, mrb.str_new(b"k").as_value()).to_string(&mrb),
            "v"
        );
        assert!(hash.get(&mrb, mrb.str_new(b"absent").as_value()).is_nil());
    }

    #[test]
    fn keys_returns_the_typed_key_array() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let hash = mrb.hash_new();

        hash.set(
            &mrb,
            mrb.str_new(b"k").as_value(),
            mrb.str_new(b"v").as_value(),
        );
        let keys = hash.keys(&mrb);

        assert_eq!(keys.entry(0).to_string(&mrb), "k");
        assert!(keys.entry(1).is_nil());
    }
}
