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

use crate::{Array, Error, Mrb, Value};
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
    /// Assigning into a frozen hash raises `FrozenError`, and storing a
    /// key runs its Ruby `hash`/`eql?` which may raise; the call runs
    /// under `Mrb::protect`, so either surfaces as `Err` rather than
    /// long-jumping.
    #[inline]
    pub fn set(self, mrb: &Mrb, key: Value, val: Value) -> Result<(), Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame; `self`
                // is Hash-tagged by the `from_value_unchecked` contract;
                // `key` and `val` originate from the same VM.
                // `mrb_hash_set` calls `hash_modify` (raises `FrozenError`
                // on a frozen hash) and may run the key's `hash`/`eql?` —
                // either caught by `protect` into `Err`.
                unsafe {
                    sys::mrb_hash_set(mrb.as_ptr(), self.0.as_raw(), key.as_raw(), val.as_raw())
                };
                Value::nil()
            })
            .map(|_| ())
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

    /// `mrb_hash_values(mrb, self)` — the values as a typed `Array`,
    /// Ruby's `Hash#values`. Mirror of `keys`; a pure read that never
    /// fails.
    #[inline]
    pub fn values(self, mrb: &Mrb) -> Array {
        #[cfg(mruby_linked)]
        {
            // SAFETY: as `keys`; `mrb_hash_values` always returns an
            // Array-tagged value, so the unchecked wrap is sound.
            unsafe {
                Array::from_value_unchecked(Value::from_raw(sys::mrb_hash_values(
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

    /// `mrb_hash_size(mrb, self)` — the number of entries, Ruby's
    /// `Hash#size`. An entry count is never negative, so it is returned
    /// as `usize`. A pure read that never fails.
    #[inline]
    pub fn len(self, mrb: &Mrb) -> usize {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is Hash-tagged by the contract; `mrb_hash_size`
            // reads only the entry count.
            (unsafe { sys::mrb_hash_size(mrb.as_ptr(), self.0.as_raw()) }) as usize
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// TRUE when the hash holds no entries.
    #[inline]
    pub fn is_empty(self, mrb: &Mrb) -> bool {
        self.len(mrb) == 0
    }

    /// `mrb_hash_key_p(mrb, self, key)` — whether `key` is present,
    /// Ruby's `Hash#key?`. Testing a key runs its `hash`/`eql?`, which
    /// may raise; the call runs under `Mrb::protect`, so that surfaces as
    /// `Err`.
    #[inline]
    pub fn contains_key(self, mrb: &Mrb, key: Value) -> Result<bool, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive in the protect frame; `self` is
                // Hash-tagged and `key` shares the VM. `mrb_hash_key_p`
                // runs the key's `hash`/`eql?` and may raise — caught by
                // `protect`.
                let present =
                    unsafe { sys::mrb_hash_key_p(mrb.as_ptr(), self.0.as_raw(), key.as_raw()) };
                if present {
                    Value::true_()
                } else {
                    Value::false_()
                }
            })
            .map(|v| v.to_bool())
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, key);
            crate::not_linked()
        }
    }

    /// `mrb_hash_fetch(mrb, self, key, default)` — the value for `key`,
    /// or `default` when absent, like Ruby's `Hash#fetch(key, default)`.
    /// The lookup runs the key's `hash`/`eql?`, which may raise; the call
    /// runs under `Mrb::protect`, so that surfaces as `Err`.
    #[inline]
    pub fn fetch(self, mrb: &Mrb, key: Value, default: Value) -> Result<Value, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: as `contains_key`; `mrb_hash_fetch` runs the key's
                // `hash`/`eql?` and may raise — caught by `protect`.
                Value::from_raw(unsafe {
                    sys::mrb_hash_fetch(
                        mrb.as_ptr(),
                        self.0.as_raw(),
                        key.as_raw(),
                        default.as_raw(),
                    )
                })
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, key, default);
            crate::not_linked()
        }
    }

    /// `mrb_hash_delete_key(mrb, self, key)` — remove `key` and return
    /// its former value, or `nil` when absent, Ruby's `Hash#delete`.
    /// Deletion mutates the hash and runs the key's `hash`/`eql?`; a
    /// frozen receiver or a raising key surfaces as `Err`.
    #[inline]
    pub fn delete(self, mrb: &Mrb, key: Value) -> Result<Value, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: as `set`; `mrb_hash_delete_key` modifies the hash
                // (raises `FrozenError` when frozen) and runs the key's
                // `hash`/`eql?` — caught by `protect`.
                Value::from_raw(unsafe {
                    sys::mrb_hash_delete_key(mrb.as_ptr(), self.0.as_raw(), key.as_raw())
                })
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, key);
            crate::not_linked()
        }
    }

    /// `mrb_hash_merge(mrb, self, other)` — fold `other`'s entries into
    /// this hash, Ruby's `Hash#update`. Merging mutates the receiver and
    /// runs each key's `hash`/`eql?`; a frozen receiver or a raising key
    /// surfaces as `Err`.
    #[inline]
    pub fn update(self, mrb: &Mrb, other: Hash) -> Result<(), Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: as `set`; `self` and `other` are Hash-tagged and
                // share the VM. `mrb_hash_merge` modifies `self` (raises
                // `FrozenError` when frozen) and runs each key's
                // `hash`/`eql?` — caught by `protect`.
                unsafe { sys::mrb_hash_merge(mrb.as_ptr(), self.0.as_raw(), other.0.as_raw()) };
                Value::nil()
            })
            .map(|_| ())
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, other);
            crate::not_linked()
        }
    }

    /// `mrb_hash_clear(mrb, self)` — remove all entries, Ruby's
    /// `Hash#clear`. Clearing a frozen hash raises `FrozenError`,
    /// surfaced here as `Err`.
    #[inline]
    pub fn clear(self, mrb: &Mrb) -> Result<(), Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame; `self` is
                // Hash-tagged by the `from_value_unchecked` contract.
                // `mrb_hash_clear` calls `hash_modify`, which raises
                // `FrozenError` on a frozen hash — caught by `protect` into
                // `Err`.
                unsafe { sys::mrb_hash_clear(mrb.as_ptr(), self.0.as_raw()) };
                Value::nil()
            })
            .map(|_| ())
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// `mrb_hash_dup(mrb, self)` — a shallow copy, Ruby's `Hash#dup`. It
    /// does not mutate the receiver, so it never fails.
    #[inline]
    pub fn dup(self, mrb: &Mrb) -> Hash {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is Hash-tagged by the contract; `mrb_hash_dup`
            // returns a fresh Hash-tagged value, so the unchecked wrap is
            // sound.
            unsafe {
                Hash::from_value_unchecked(Value::from_raw(sys::mrb_hash_dup(
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
        )
        .expect("assigning into a fresh hash succeeds");

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
        )
        .expect("assigning into a fresh hash succeeds");
        let keys = hash.keys(&mrb);

        assert_eq!(keys.entry(0).to_string(&mrb), "k");
        assert!(keys.entry(1).is_nil());
    }

    #[test]
    fn set_surfaces_frozen_receiver_as_err() {
        use crate::{Ccontext, Error, FromValue, Hash};

        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt =
            Ccontext::new(&mrb, c"frozen_hash.rb").expect("allocating the context must succeed");

        // A frozen Hash still carries the Hash tag, so the downcast holds,
        // but assigning into it raises FrozenError — which protect catches
        // into Err rather than long-jumping.
        let frozen = Hash::from_value(cxt.load_nstring(b"{}.freeze"))
            .expect("a frozen Hash literal is Hash-tagged");
        assert!(matches!(
            frozen.set(
                &mrb,
                mrb.str_new(b"k").as_value(),
                mrb.str_new(b"v").as_value()
            ),
            Err(Error::Exception(_))
        ));
    }

    #[test]
    fn values_size_and_emptiness_read_the_structure() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let hash = mrb.hash_new();
        assert!(hash.is_empty(&mrb));
        assert_eq!(hash.len(&mrb), 0);

        hash.set(
            &mrb,
            mrb.str_new(b"k").as_value(),
            mrb.str_new(b"v").as_value(),
        )
        .expect("set succeeds");

        assert_eq!(hash.len(&mrb), 1);
        assert!(!hash.is_empty(&mrb));
        assert_eq!(hash.values(&mrb).entry(0).to_string(&mrb), "v");
    }

    #[test]
    fn contains_key_and_fetch_read_by_key() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let hash = mrb.hash_new();
        let k = mrb.str_new(b"k").as_value();
        hash.set(&mrb, k, mrb.str_new(b"v").as_value())
            .expect("set succeeds");

        assert!(hash.contains_key(&mrb, k).expect("key test succeeds"));
        assert!(!hash
            .contains_key(&mrb, mrb.str_new(b"absent").as_value())
            .expect("key test succeeds"));

        assert_eq!(
            hash.fetch(&mrb, k, mrb.str_new(b"def").as_value())
                .expect("fetch succeeds")
                .to_string(&mrb),
            "v"
        );
        // An absent key returns the supplied default, not an error.
        assert_eq!(
            hash.fetch(
                &mrb,
                mrb.str_new(b"absent").as_value(),
                mrb.str_new(b"def").as_value(),
            )
            .expect("fetch succeeds")
            .to_string(&mrb),
            "def"
        );
    }

    #[test]
    fn delete_removes_and_update_merges() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let hash = mrb.hash_new();
        let k = mrb.str_new(b"k").as_value();
        hash.set(&mrb, k, mrb.str_new(b"v").as_value())
            .expect("set succeeds");

        assert_eq!(
            hash.delete(&mrb, k)
                .expect("delete succeeds")
                .to_string(&mrb),
            "v"
        );
        assert!(hash.is_empty(&mrb));

        let other = mrb.hash_new();
        other
            .set(
                &mrb,
                mrb.str_new(b"a").as_value(),
                mrb.str_new(b"1").as_value(),
            )
            .expect("set succeeds");
        hash.update(&mrb, other).expect("update succeeds");
        assert_eq!(
            hash.get(&mrb, mrb.str_new(b"a").as_value()).to_string(&mrb),
            "1"
        );
    }

    #[test]
    fn clear_empties_the_hash() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let hash = mrb.hash_new();
        hash.set(
            &mrb,
            mrb.str_new(b"k").as_value(),
            mrb.str_new(b"v").as_value(),
        )
        .expect("set succeeds");
        assert!(!hash.is_empty(&mrb));

        hash.clear(&mrb).expect("clear succeeds");
        assert!(hash.is_empty(&mrb));
    }

    #[test]
    fn clear_surfaces_frozen_receiver_as_err() {
        use crate::{Ccontext, Error, FromValue, Hash};

        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt =
            Ccontext::new(&mrb, c"frozen_clear.rb").expect("allocating the context must succeed");

        // clear checks frozen state before touching entries, so even a
        // populated frozen hash surfaces FrozenError as Err.
        let frozen = Hash::from_value(cxt.load_nstring(b"{a: 1}.freeze"))
            .expect("a frozen Hash literal is Hash-tagged");
        assert!(matches!(frozen.clear(&mrb), Err(Error::Exception(_))));
    }

    #[test]
    fn dup_copies_independently() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let hash = mrb.hash_new();
        hash.set(
            &mrb,
            mrb.str_new(b"k").as_value(),
            mrb.str_new(b"v").as_value(),
        )
        .expect("set succeeds");

        let copy = hash.dup(&mrb);
        hash.delete(&mrb, mrb.str_new(b"k").as_value())
            .expect("delete succeeds");

        // Mutating the original leaves the dup untouched.
        assert!(hash.is_empty(&mrb));
        assert_eq!(
            copy.get(&mrb, mrb.str_new(b"k").as_value()).to_string(&mrb),
            "v"
        );
    }

    #[test]
    fn delete_surfaces_frozen_receiver_as_err() {
        use crate::{Ccontext, Error, FromValue, Hash};

        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt =
            Ccontext::new(&mrb, c"frozen_del.rb").expect("allocating the context must succeed");

        // delete checks frozen state before touching entries, so even a
        // populated frozen hash surfaces FrozenError as Err.
        let frozen = Hash::from_value(cxt.load_nstring(b"{a: 1}.freeze"))
            .expect("a frozen Hash literal is Hash-tagged");
        assert!(matches!(
            frozen.delete(&mrb, mrb.str_new(b"a").as_value()),
            Err(Error::Exception(_))
        ));
    }

    #[test]
    fn update_surfaces_frozen_receiver_as_err() {
        use crate::{Ccontext, Error, FromValue, Hash};

        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt =
            Ccontext::new(&mrb, c"frozen_update.rb").expect("allocating the context must succeed");

        // merge checks frozen state before folding entries, so merging
        // into a frozen hash surfaces FrozenError as Err.
        let frozen = Hash::from_value(cxt.load_nstring(b"{a: 1}.freeze"))
            .expect("a frozen Hash literal is Hash-tagged");
        let other = mrb.hash_new();
        other
            .set(
                &mrb,
                mrb.str_new(b"b").as_value(),
                mrb.str_new(b"2").as_value(),
            )
            .expect("set succeeds");
        assert!(matches!(
            frozen.update(&mrb, other),
            Err(Error::Exception(_))
        ));
    }
}
