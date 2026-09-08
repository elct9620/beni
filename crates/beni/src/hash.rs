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
