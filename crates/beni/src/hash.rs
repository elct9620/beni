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

/// Signal a `Hash::each` closure returns to steer the walk. Mirrors
/// magnus's `ForEach`, minus its CRuby-only `Delete` (mruby's
/// `mrb_hash_foreach` has no delete-and-continue path).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ForEach {
    /// Visit the remaining pairs.
    Continue,
    /// End the walk before the remaining pairs.
    Stop,
}

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

    /// `mrb_hash_get(mrb, self, key)` — the value for `key`, or `nil`
    /// when absent. The lookup runs the key's `hash`/`eql?`, and an
    /// absent key runs the hash's `default`; either may raise, so the
    /// call runs under `Mrb::protect` and surfaces that as `Err`.
    #[inline]
    pub fn get(self, mrb: &Mrb, key: Value) -> Result<Value, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: as `contains_key`; `mrb_hash_get` runs the key's
                // `hash`/`eql?` and an absent-key `default` lookup, both of
                // which may raise — caught by `protect`.
                Value::from_raw(unsafe {
                    sys::mrb_hash_get(mrb.as_ptr(), self.0.as_raw(), key.as_raw())
                })
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

    /// `mrb_hash_empty_p(mrb, self)` — TRUE when the hash holds no
    /// entries, Ruby's `Hash#empty?`. A pure read that never fails.
    #[inline]
    pub fn is_empty(self, mrb: &Mrb) -> bool {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is Hash-tagged by the contract; `mrb_hash_empty_p`
            // reads only the entry count.
            unsafe { sys::mrb_hash_empty_p(mrb.as_ptr(), self.0.as_raw()) }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
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

    /// `mrb_hash_foreach(mrb, self, …)` — visit each `(key, value)` pair
    /// in insertion order, handing both to `body`. Returning
    /// `ForEach::Stop` ends the walk before the remaining pairs;
    /// `ForEach::Continue` proceeds. Mirrors magnus's `RHash::foreach`,
    /// narrowed to the continue/stop signal mruby's C foreach supports.
    ///
    /// The walk dispatches no Ruby of its own, but a `body` that
    /// re-enters the VM to mutate this hash's table trips mruby's in-walk
    /// modification guard, which surfaces here as `Err` carrying the
    /// `RuntimeError` mruby raises — the call runs under `Mrb::protect`,
    /// so that raise is caught rather than long-jumping.
    ///
    /// A panic in `body` is caught at the FFI boundary, stops the walk,
    /// and resurfaces here once the walk unwinds — it never unwinds into
    /// mruby's C frames.
    #[inline]
    pub fn each<F>(self, mrb: &Mrb, body: F) -> Result<(), Error>
    where
        F: FnMut(Value, Value) -> ForEach,
    {
        #[cfg(mruby_linked)]
        {
            // Park the closure beside a panic slot in a stack local. The
            // trampoline borrows it per pair; on a panic it stashes the
            // unwind payload here and reports `Stop`, so the C walk ends
            // without a panic crossing its frames. The payload resumes
            // below once control is back on the Rust side.
            struct Walk<F> {
                body: F,
                panic: Option<Box<dyn std::any::Any + Send>>,
            }

            unsafe extern "C" fn trampoline<F>(
                _mrb: *mut sys::mrb_state,
                key: sys::mrb_value,
                val: sys::mrb_value,
                data: *mut core::ffi::c_void,
            ) -> core::ffi::c_int
            where
                F: FnMut(Value, Value) -> ForEach,
            {
                // SAFETY: `data` is the `&mut Walk<F>` handed to
                // `mrb_hash_foreach` below; the foreach call borrows it
                // for the duration of the walk on this same thread.
                let walk: &mut Walk<F> = unsafe { &mut *(data as *mut Walk<F>) };
                let key = Value::from_raw(key);
                let val = Value::from_raw(val);
                // Catch here so a `body` panic stops the walk instead of
                // unwinding through `mrb_hash_foreach`'s C frame.
                // AssertUnwindSafe matches the crate's other panic
                // boundaries: the parked payload is the only state that
                // survives the catch. A non-zero return stops the C walk,
                // so the trampoline is not re-entered after `Stop` or a
                // parked panic.
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    (walk.body)(key, val)
                })) {
                    Ok(ForEach::Continue) => 0,
                    Ok(ForEach::Stop) => 1,
                    Err(payload) => {
                        walk.panic = Some(payload);
                        1
                    }
                }
            }

            let mut walk = Walk { body, panic: None };
            // Run the whole walk under `protect`: `H_CHECK_MODIFIED` raises
            // `RuntimeError` when `body` re-enters the VM and mutates this
            // hash mid-walk, and that raise long-jumps out of
            // `mrb_hash_foreach`. The protect frame catches it into `Err`.
            let walk_ptr = &mut walk as *mut Walk<F> as *mut core::ffi::c_void;
            let result = mrb.protect(|mrb| {
                // SAFETY: `self` is Hash-tagged by the
                // `from_value_unchecked` contract, so the object pointer is
                // an `RHash`; both `mrb_obj_ptr_func` and the C
                // `mrb_hash_ptr` macro read the same union pointer,
                // differing only in the cast. `mrb` is alive inside the
                // protect frame; `trampoline::<F>` upholds the
                // `mrb_hash_foreach_func` ABI; `walk_ptr` points to `walk`
                // on this frame, which outlives the call. bindgen wraps the
                // function-typedef parameter in `Option`, so the trampoline
                // is passed via `Some`.
                unsafe {
                    let hash = sys::mrb_obj_ptr_func(self.0.as_raw()) as *mut sys::RHash;
                    sys::mrb_hash_foreach(mrb.as_ptr(), hash, Some(trampoline::<F>), walk_ptr);
                }
                Value::nil()
            });
            // A `body` panic and an mruby raise cannot both fire in one
            // callback, but each leaves its own channel: resurface a parked
            // panic first (it preempts any `Err`), then return protect's
            // Result for the modify-raise path.
            if let Some(payload) = walk.panic {
                std::panic::resume_unwind(payload);
            }
            result.map(|_| ())
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, body);
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
            hash.get(&mrb, mrb.str_new(b"k").as_value())
                .expect("a present string key reads without raising")
                .to_string(&mrb),
            "v"
        );
        assert!(hash
            .get(&mrb, mrb.str_new(b"absent").as_value())
            .expect("an absent string key reads without raising")
            .is_nil());
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
    fn keyed_operations_surface_a_raising_key_as_err() {
        use crate::{Ccontext, Error, Value};

        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt =
            Ccontext::new(&mrb, c"raising_key.rb").expect("allocating the context must succeed");

        // mruby locates a key by dispatching its `hash` and `eql?`; a key
        // that raises in both drives every keyed operation through the
        // dispatch protect must catch rather than long-jump. A seeded
        // entry forces the comparison to run even on a small hash.
        let key = cxt.load_nstring(
            b"class BeniBoomKey; def hash; raise 'no'; end; def eql?(o); raise 'no'; end; end; BeniBoomKey.new",
        );
        assert!(
            mrb.pending_exc().is_nil(),
            "defining the key class must not raise"
        );

        let hash = mrb.hash_new();
        hash.set(&mrb, mrb.str_new(b"seed").as_value(), Value::nil())
            .expect("seeding a plain key does not raise");

        let v = mrb.str_new(b"v").as_value();
        assert!(matches!(hash.set(&mrb, key, v), Err(Error::Exception(_))));
        assert!(matches!(hash.get(&mrb, key), Err(Error::Exception(_))));
        assert!(matches!(
            hash.contains_key(&mrb, key),
            Err(Error::Exception(_))
        ));
        assert!(matches!(
            hash.fetch(&mrb, key, Value::nil()),
            Err(Error::Exception(_))
        ));
        assert!(matches!(hash.delete(&mrb, key), Err(Error::Exception(_))));
    }

    #[test]
    fn read_surfaces_a_raising_default_as_err() {
        use crate::{Ccontext, Error, FromValue, Hash};

        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt = Ccontext::new(&mrb, c"raising_default.rb")
            .expect("allocating the context must succeed");

        // A hash whose default block raises turns an absent-key read into
        // a raise protect must catch — the default path a read takes and
        // fetch does not.
        let hash = Hash::from_value(cxt.load_nstring(b"Hash.new { raise 'no' }"))
            .expect("a Hash is Hash-tagged");
        assert!(
            mrb.pending_exc().is_nil(),
            "building the hash must not raise"
        );

        assert!(matches!(
            hash.get(&mrb, mrb.str_new(b"absent").as_value()),
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
            hash.get(&mrb, mrb.str_new(b"a").as_value())
                .expect("a present string key reads without raising")
                .to_string(&mrb),
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
            copy.get(&mrb, mrb.str_new(b"k").as_value())
                .expect("a present string key reads without raising")
                .to_string(&mrb),
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

    #[test]
    fn each_visits_every_pair_in_insertion_order() {
        use crate::{ForEach, Value};

        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let hash = mrb.hash_new();
        hash.set(&mrb, mrb.str_new(b"a").as_value(), Value::from_int(&mrb, 1))
            .expect("set succeeds");
        hash.set(&mrb, mrb.str_new(b"b").as_value(), Value::from_int(&mrb, 2))
            .expect("set succeeds");
        hash.set(&mrb, mrb.str_new(b"c").as_value(), Value::from_int(&mrb, 3))
            .expect("set succeeds");

        let mut seen = Vec::new();
        hash.each(&mrb, |key, val| {
            seen.push((key.to_string(&mrb), val.to_string(&mrb)));
            ForEach::Continue
        })
        .expect("a read-only walk does not raise");

        assert_eq!(
            seen,
            vec![
                ("a".to_owned(), "1".to_owned()),
                ("b".to_owned(), "2".to_owned()),
                ("c".to_owned(), "3".to_owned()),
            ]
        );
    }

    #[test]
    fn each_stops_early_on_stop() {
        use crate::{ForEach, Value};

        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let hash = mrb.hash_new();
        hash.set(&mrb, mrb.str_new(b"a").as_value(), Value::from_int(&mrb, 1))
            .expect("set succeeds");
        hash.set(&mrb, mrb.str_new(b"b").as_value(), Value::from_int(&mrb, 2))
            .expect("set succeeds");
        hash.set(&mrb, mrb.str_new(b"c").as_value(), Value::from_int(&mrb, 3))
            .expect("set succeeds");

        // Stopping at the first pair leaves the rest unvisited.
        let mut count = 0;
        hash.each(&mrb, |_, _| {
            count += 1;
            ForEach::Stop
        })
        .expect("an early-stopping walk does not raise");

        assert_eq!(count, 1);
    }

    #[test]
    fn each_surfaces_an_in_walk_modification_as_err() {
        use crate::{Error, ForEach, Value};

        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let hash = mrb.hash_new();
        hash.set(&mrb, mrb.str_new(b"a").as_value(), Value::from_int(&mrb, 1))
            .expect("set succeeds");
        hash.set(&mrb, mrb.str_new(b"b").as_value(), Value::from_int(&mrb, 2))
            .expect("set succeeds");

        // A closure that re-enters the VM to clear the hash it is walking
        // resets the entry table, so the guard mruby runs before the next
        // callback raises RuntimeError. protect catches that into Err
        // rather than letting it long-jump across mrb_hash_foreach's FFI
        // frame.
        let result = hash.each(&mrb, |_, _| {
            hash.clear(&mrb)
                .expect("the in-walk clear itself does not raise");
            ForEach::Continue
        });
        assert!(matches!(result, Err(Error::Exception(_))));

        // The VM survives the caught raise and stays usable: a fresh
        // operation runs without crashing.
        let other = mrb.hash_new();
        other
            .set(&mrb, mrb.str_new(b"x").as_value(), Value::from_int(&mrb, 9))
            .expect("the VM is usable after the protected raise");
        assert_eq!(other.len(&mrb), 1);
    }

    #[test]
    fn each_resurfaces_a_closure_panic_on_the_rust_side() {
        use crate::Value;

        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let hash = mrb.hash_new();
        hash.set(&mrb, mrb.str_new(b"a").as_value(), Value::from_int(&mrb, 1))
            .expect("set succeeds");
        hash.set(&mrb, mrb.str_new(b"b").as_value(), Value::from_int(&mrb, 2))
            .expect("set succeeds");

        // A panic in the closure is caught at the FFI boundary, stops the
        // walk, and resumes here once mrb_hash_foreach returns — never
        // unwinding through mruby's C frames. catch_unwind sees the
        // resumed panic, proving it crossed back to the Rust side intact.
        let visited = std::cell::Cell::new(0u32);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // The closure panic resurfaces before `each` returns a Result,
            // so the value is never produced — bind it to silence must_use.
            let _ = hash.each(&mrb, |_, _| {
                visited.set(visited.get() + 1);
                panic!("boom in each closure");
            });
        }));

        let payload = result.expect_err("the closure panic must resurface Rust-side");
        let msg = payload
            .downcast_ref::<&str>()
            .copied()
            .expect("the original panic payload survives the round-trip");
        assert_eq!(msg, "boom in each closure");
        // The walk stopped at the first pair rather than running on.
        assert_eq!(visited.get(), 1);

        // The VM survives the caught panic.
        assert_eq!(hash.len(&mrb), 2);
    }
}
