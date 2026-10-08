//! Typed `RHash` newtype around a Hash-tagged `Value`.
//!
//! `RHash` is `#[repr(transparent)]` over `Value` (which is itself
//! `#[repr(transparent)]` over `mrb_value`). The two share their
//! in-memory layout — `RHash` is exactly an `mrb_value` known to carry
//! an mruby `Hash`. Construction is by explicit unchecked cast from
//! `Value`; element operations cluster on the resulting newtype.
//!
//! Mirrors magnus's `src/r_hash.rs`: factories live on `Ruby` /
//! `Mrb`, per-hash ops (`set`, `get`, `keys`) live here.

use crate::{
    sys::AsRawValue, Error, FromValue, Mrb, RArray, ReprValue, TryConvert, TryConvertOwned, Value,
};
use beni_sys as sys;

/// Signal an `RHash::foreach` closure returns to steer the walk. Mirrors
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
/// `FromValue` downcast (`RHash::from_value`, tag-discriminated), or
/// `RHash::from_value_unchecked` (assert that a `Value` you
/// already hold is Hash-tagged). Round-trip back to a generic
/// `Value` via `ReprValue::as_value` for APIs that take any value.
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct RHash(pub(crate) Value);

impl RHash {
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

    /// `mrb_hash_set(mrb, self, key, val)` — assign `key => val`.
    /// Assigning into a frozen hash raises `FrozenError`, and storing a
    /// key runs its Ruby `hash`/`eql?` which may raise; the call runs
    /// under exception protection, so either surfaces as `Err` rather than
    /// long-jumping.
    #[inline]
    pub fn set(self, mrb: &Mrb, key: Value, val: Value) -> Result<(), Error> {
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame; `self`
            // is Hash-tagged by the `from_value_unchecked` contract;
            // `key` and `val` originate from the same VM.
            // `mrb_hash_set` calls `hash_modify` (raises `FrozenError`
            // on a frozen hash) and may run the key's `hash`/`eql?` —
            // either caught by `protect` into `Err`.
            unsafe { sys::mrb_hash_set(mrb.as_ptr(), self.0.as_raw(), key.as_raw(), val.as_raw()) };
            crate::value::qnil()
        })
        .map(|_| ())
    }

    /// `mrb_hash_get(mrb, self, key)` — the value for `key`, or `nil`
    /// when absent. The lookup runs the key's `hash`/`eql?`, and an
    /// absent key runs the hash's `default`; either may raise, so the
    /// call runs under exception protection and surfaces that as `Err`.
    #[inline]
    pub fn get(self, mrb: &Mrb, key: Value) -> Result<Value, Error> {
        mrb.protect(|mrb| {
            // SAFETY: as `contains_key`; `mrb_hash_get` runs the key's
            // `hash`/`eql?` and an absent-key `default` lookup, both of
            // which may raise — caught by `protect`.
            Value::from_raw_unchecked(unsafe {
                sys::mrb_hash_get(mrb.as_ptr(), self.0.as_raw(), key.as_raw())
            })
        })
    }

    /// `mrb_hash_keys(mrb, self)` — return the Array of keys as a
    /// typed `RArray`.
    #[inline]
    pub fn keys(self, mrb: &Mrb) -> RArray {
        // SAFETY: as `set`; `mrb_hash_keys` always returns an
        // Array-tagged value, so the unchecked wrap is sound.
        unsafe {
            RArray::from_value_unchecked(Value::from_raw_unchecked(sys::mrb_hash_keys(
                mrb.as_ptr(),
                self.0.as_raw(),
            )))
        }
    }

    /// `mrb_hash_values(mrb, self)` — the values as a typed `RArray`,
    /// Ruby's `Hash#values`. Mirror of `keys`; a pure read that never
    /// fails.
    #[inline]
    pub fn values(self, mrb: &Mrb) -> RArray {
        // SAFETY: as `keys`; `mrb_hash_values` always returns an
        // Array-tagged value, so the unchecked wrap is sound.
        unsafe {
            RArray::from_value_unchecked(Value::from_raw_unchecked(sys::mrb_hash_values(
                mrb.as_ptr(),
                self.0.as_raw(),
            )))
        }
    }

    /// `mrb_hash_size(mrb, self)` — the number of entries, Ruby's
    /// `Hash#size`. An entry count is never negative, so it is returned
    /// as `usize`. A pure read that never fails.
    #[inline]
    pub fn len(self, mrb: &Mrb) -> usize {
        // SAFETY: `self` is Hash-tagged by the contract; `mrb_hash_size`
        // reads only the entry count.
        (unsafe { sys::mrb_hash_size(mrb.as_ptr(), self.0.as_raw()) }) as usize
    }

    /// `mrb_hash_empty_p(mrb, self)` — TRUE when the hash holds no
    /// entries, Ruby's `Hash#empty?`. A pure read that never fails.
    #[inline]
    pub fn is_empty(self, mrb: &Mrb) -> bool {
        // SAFETY: `self` is Hash-tagged by the contract; `mrb_hash_empty_p`
        // reads only the entry count.
        unsafe { sys::mrb_hash_empty_p(mrb.as_ptr(), self.0.as_raw()) }
    }

    /// `mrb_hash_key_p(mrb, self, key)` — whether `key` is present,
    /// Ruby's `Hash#key?`. Testing a key runs its `hash`/`eql?`, which
    /// may raise; the call runs under exception protection, so that surfaces as
    /// `Err`.
    #[inline]
    pub fn contains_key(self, mrb: &Mrb, key: Value) -> Result<bool, Error> {
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive in the protect frame; `self` is
            // Hash-tagged and `key` shares the VM. `mrb_hash_key_p`
            // runs the key's `hash`/`eql?` and may raise — caught by
            // `protect`.
            let present =
                unsafe { sys::mrb_hash_key_p(mrb.as_ptr(), self.0.as_raw(), key.as_raw()) };
            if present {
                crate::value::qtrue().as_value()
            } else {
                crate::value::qfalse().as_value()
            }
        })
        .map(|v| v.to_bool())
    }

    /// `mrb_hash_fetch(mrb, self, key, default)` — the value for `key`,
    /// or `default` when absent, like Ruby's `Hash#fetch(key, default)`.
    /// The lookup runs the key's `hash`/`eql?`, which may raise; the call
    /// runs under exception protection, so that surfaces as `Err`.
    #[inline]
    pub fn fetch(self, mrb: &Mrb, key: Value, default: Value) -> Result<Value, Error> {
        mrb.protect(|mrb| {
            // SAFETY: as `contains_key`; `mrb_hash_fetch` runs the key's
            // `hash`/`eql?` and may raise — caught by `protect`.
            Value::from_raw_unchecked(unsafe {
                sys::mrb_hash_fetch(
                    mrb.as_ptr(),
                    self.0.as_raw(),
                    key.as_raw(),
                    default.as_raw(),
                )
            })
        })
    }

    /// `mrb_hash_delete_key(mrb, self, key)` — remove `key` and return
    /// its former value, or `nil` when absent, Ruby's `Hash#delete`.
    /// Deletion mutates the hash and runs the key's `hash`/`eql?`; a
    /// frozen receiver or a raising key surfaces as `Err`.
    #[inline]
    pub fn delete(self, mrb: &Mrb, key: Value) -> Result<Value, Error> {
        mrb.protect(|mrb| {
            // SAFETY: as `set`; `mrb_hash_delete_key` modifies the hash
            // (raises `FrozenError` when frozen) and runs the key's
            // `hash`/`eql?` — caught by `protect`.
            Value::from_raw_unchecked(unsafe {
                sys::mrb_hash_delete_key(mrb.as_ptr(), self.0.as_raw(), key.as_raw())
            })
        })
    }

    /// `mrb_hash_merge(mrb, self, other)` — fold `other`'s entries into
    /// this hash, Ruby's `Hash#update`. Merging mutates the receiver and
    /// runs each key's `hash`/`eql?`; a frozen receiver or a raising key
    /// surfaces as `Err`.
    #[inline]
    pub fn update(self, mrb: &Mrb, other: RHash) -> Result<(), Error> {
        mrb.protect(|mrb| {
            // SAFETY: as `set`; `self` and `other` are Hash-tagged and
            // share the VM. `mrb_hash_merge` modifies `self` (raises
            // `FrozenError` when frozen) and runs each key's
            // `hash`/`eql?` — caught by `protect`.
            unsafe { sys::mrb_hash_merge(mrb.as_ptr(), self.0.as_raw(), other.0.as_raw()) };
            crate::value::qnil()
        })
        .map(|_| ())
    }

    /// `mrb_hash_clear(mrb, self)` — remove all entries, Ruby's
    /// `Hash#clear`. Clearing a frozen hash raises `FrozenError`,
    /// surfaced here as `Err`.
    #[inline]
    pub fn clear(self, mrb: &Mrb) -> Result<(), Error> {
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame; `self` is
            // Hash-tagged by the `from_value_unchecked` contract.
            // `mrb_hash_clear` calls `hash_modify`, which raises
            // `FrozenError` on a frozen hash — caught by `protect` into
            // `Err`.
            unsafe { sys::mrb_hash_clear(mrb.as_ptr(), self.0.as_raw()) };
            crate::value::qnil()
        })
        .map(|_| ())
    }

    /// `mrb_hash_dup(mrb, self)` — a shallow copy, Ruby's `Hash#dup`. It
    /// does not mutate the receiver, so it never fails.
    #[inline]
    pub fn dup(self, mrb: &Mrb) -> RHash {
        // SAFETY: `self` is Hash-tagged by the contract; `mrb_hash_dup`
        // returns a fresh Hash-tagged value, so the unchecked wrap is
        // sound.
        unsafe {
            RHash::from_value_unchecked(Value::from_raw_unchecked(sys::mrb_hash_dup(
                mrb.as_ptr(),
                self.0.as_raw(),
            )))
        }
    }

    /// `mrb_hash_foreach(mrb, self, …)` — visit each pair in insertion
    /// order, converted through `TryConvert` into the types `func` takes.
    /// `func` answers `ForEach::Continue` to proceed, `ForEach::Stop` to
    /// end the walk, or an `Err`, which ends it and surfaces here. Mirrors
    /// magnus's `RHash::foreach`, without its `Delete`: mruby's walk has no
    /// path to delete the pair it stands on.
    ///
    /// A pair that fails its conversion ends the walk with that `Err`, and
    /// a panic in `func` or a conversion ends it with an `Err` carrying the
    /// panic's message, never unwinding into mruby's frames. The walk
    /// dispatches no Ruby of its own. A visit — a pair's conversion and
    /// the `func` call on it — after which the hash holds a different
    /// number of pairs ends the walk with an `Err` carrying `RuntimeError`
    /// "hash modified", unless `func` answered an `Err` or panicked on that
    /// visit. A visit that changes pairs but not their number may end the
    /// walk the same way or let it continue over unspecified pairs, each
    /// one the hash held during the walk. Each pair handed to `func` stays
    /// reachable as every value that crosses out does, even once this hash
    /// lets it go.
    #[inline]
    pub fn foreach<F, K, V>(self, mrb: &Mrb, mut func: F) -> Result<(), Error>
    where
        F: FnMut(K, V) -> Result<ForEach, Error>,
        K: TryConvert,
        V: TryConvert,
    {
        self.walk(mrb, Some(self.pair_buffer(mrb)), |key, val| {
            func(K::try_convert(key, mrb)?, V::try_convert(val, mrb)?)
        })
    }

    /// The pairs, each key and value converted through `TryConvert`, as a
    /// Rust hash map, or the first `Err`. Mirrors magnus's
    /// `RHash::to_hash_map`.
    pub fn to_hash_map<K, V>(self, mrb: &Mrb) -> Result<std::collections::HashMap<K, V>, Error>
    where
        K: TryConvertOwned + Eq + core::hash::Hash,
        V: TryConvertOwned,
    {
        self.converted_pairs(mrb)
    }

    /// The pairs, each key and value converted through `TryConvert`, as a
    /// Rust ordered map, or the first `Err`. Mirrors magnus's
    /// `RHash::to_btree_map`.
    pub fn to_btree_map<K, V>(self, mrb: &Mrb) -> Result<std::collections::BTreeMap<K, V>, Error>
    where
        K: TryConvertOwned + Ord,
        V: TryConvertOwned,
    {
        self.converted_pairs(mrb)
    }

    /// Convert each pair as the walk visits it. The converted pair holds no
    /// `Value`, so the pair holds the arena only while it converts.
    fn converted_pairs<K, V, C>(self, mrb: &Mrb) -> Result<C, Error>
    where
        K: TryConvertOwned,
        V: TryConvertOwned,
        C: Default + Extend<(K, V)>,
    {
        let mut pairs = C::default();
        self.walk(mrb, None, |key, val| {
            let _held_while_converting = mrb.arena_scope();
            let key = K::try_convert(mrb.hold(key), mrb)?;
            let val = V::try_convert(mrb.hold(val), mrb)?;
            pairs.extend(core::iter::once((key, val)));
            Ok(ForEach::Continue)
        })?;
        Ok(pairs)
    }

    /// An empty Array with room for every pair, key then value.
    fn pair_buffer(self, mrb: &Mrb) -> RArray {
        mrb.ary_new_capa(2 * self.len(mrb))
    }

    /// Run `visit` over each pair, pushing each pair into `held` first when
    /// one is given, and end the walk with "hash modified" when a visit
    /// changes the pair count. A `visit` `Err` or panic stops the walk and
    /// outranks that error.
    fn walk<F>(self, mrb: &Mrb, held: Option<RArray>, visit: F) -> Result<(), Error>
    where
        F: FnMut(Value, Value) -> Result<ForEach, Error>,
    {
        #[cfg(mruby_lt_4_1)]
        let walked = self.walk_snapshot(mrb, held.unwrap_or_else(|| self.pair_buffer(mrb)), visit);
        #[cfg(not(mruby_lt_4_1))]
        let walked = self.walk_live(mrb, held, visit);
        walked
    }

    /// Walk a copy of the pairs, checking the count around each visit as
    /// mruby 4.1's own walk does. mruby before 4.1 counts its walk down
    /// from the size it read at the start, so a visit that deletes a pair
    /// ahead of it would send the live walk past the hash's entries. The
    /// copy lands in `snapshot`, which keeps every pair reachable. The
    /// arena the visits grow is released when the walk returns, as the
    /// protect frame around mruby's own walk releases it.
    #[cfg(mruby_lt_4_1)]
    fn walk_snapshot<F>(self, mrb: &Mrb, snapshot: RArray, visit: F) -> Result<(), Error>
    where
        F: FnMut(Value, Value) -> Result<ForEach, Error>,
    {
        let scope = mrb.arena_scope();
        match self.visit_snapshot(mrb, snapshot, visit) {
            Err(Error::Exception(exc)) => Err(Error::Exception(scope.keep(exc))),
            walked => walked,
        }
    }

    /// Run `visit` over the copy of the pairs `snapshot` takes, ending
    /// with "hash modified" when a visit changes the pair count.
    #[cfg(mruby_lt_4_1)]
    fn visit_snapshot<F>(self, mrb: &Mrb, snapshot: RArray, mut visit: F) -> Result<(), Error>
    where
        F: FnMut(Value, Value) -> Result<ForEach, Error>,
    {
        self.walk_live(mrb, Some(snapshot), |_, _| Ok(ForEach::Continue))?;
        let count = self.len(mrb);
        for pair in 0..snapshot.len() / 2 {
            // SAFETY: the copy holds two entries per pair and nothing else
            // reaches it, so both indices are in bounds; `snapshot` sits in
            // the caller's arena, keeping both entries reachable.
            let (key, val) = unsafe {
                (
                    snapshot.entry_unheld(2 * pair),
                    snapshot.entry_unheld(2 * pair + 1),
                )
            };
            let flow = visit_one(&mut visit, key, val)?;
            if self.len(mrb) != count {
                return Err(crate::error::core_error(
                    mrb,
                    c"RuntimeError",
                    "hash modified",
                ));
            }
            if flow == ForEach::Stop {
                break;
            }
        }
        Ok(())
    }

    /// Run `visit` over each pair through mruby's own walk under exception
    /// protection, pushing each pair into `held` first when one is given.
    /// A `visit` `Err` or panic is parked and stops the walk, then surfaces
    /// once the walk returns, ahead of whatever mruby's guard raised.
    fn walk_live<F>(self, mrb: &Mrb, held: Option<RArray>, visit: F) -> Result<(), Error>
    where
        F: FnMut(Value, Value) -> Result<ForEach, Error>,
    {
        struct Walk<F> {
            visit: F,
            held: Option<RArray>,
            parked: Option<Error>,
        }

        // A raise long-jumps across this frame only from the pushes, so it
        // holds no value that needs dropping; the visit runs one frame up.
        unsafe extern "C" fn trampoline<F>(
            mrb: *mut sys::mrb_state,
            key: sys::mrb_value,
            val: sys::mrb_value,
            data: *mut core::ffi::c_void,
        ) -> core::ffi::c_int
        where
            F: FnMut(Value, Value) -> Result<ForEach, Error>,
        {
            // SAFETY: `data` is the `&mut Walk<F>` handed to
            // `mrb_hash_foreach` below, borrowed for the walk on this
            // thread.
            let walk: &mut Walk<F> = unsafe { &mut *(data as *mut Walk<F>) };
            if let Some(held) = walk.held {
                // SAFETY: `mrb` is the live state driving the walk and
                // `held` an Array from it; a push that raises long-jumps to
                // the walk's protect frame across no value needing a drop.
                unsafe {
                    sys::mrb_ary_push(mrb, held.as_raw(), key);
                    sys::mrb_ary_push(mrb, held.as_raw(), val);
                }
            }
            visit_pair(
                walk,
                Value::from_raw_unchecked(key),
                Value::from_raw_unchecked(val),
            )
        }

        fn visit_pair<F>(walk: &mut Walk<F>, key: Value, val: Value) -> core::ffi::c_int
        where
            F: FnMut(Value, Value) -> Result<ForEach, Error>,
        {
            match visit_one(&mut walk.visit, key, val) {
                Ok(ForEach::Continue) => 0,
                Ok(ForEach::Stop) => 1,
                Err(err) => {
                    walk.parked = Some(err);
                    1
                }
            }
        }

        let mut walk = Walk {
            visit,
            held,
            parked: None,
        };
        let walk_ptr = &mut walk as *mut Walk<F> as *mut core::ffi::c_void;
        // `H_CHECK_MODIFIED` raises "hash modified" when a visit moves this
        // hash's table, and from mruby 4.1 when it changes the pair count;
        // the protect frame catches that raise into `Err`.
        let result = mrb.protect(|mrb| {
            // SAFETY: `self` is Hash-tagged, so its object pointer is an
            // `RHash`; `mrb` is alive inside the protect frame;
            // `trampoline::<F>` upholds the `mrb_hash_foreach_func` ABI;
            // `walk_ptr` points to `walk` on this frame, which outlives the
            // call. bindgen wraps the callback parameter in `Option`.
            unsafe {
                let hash = sys::mrb_obj_ptr_func(self.0.as_raw()) as *mut sys::RHash;
                sys::mrb_hash_foreach(mrb.as_ptr(), hash, Some(trampoline::<F>), walk_ptr);
            }
            crate::value::qnil().as_value()
        });
        match walk.parked {
            Some(err) => {
                // The protect frame released the arena the visit held this
                // exception in; hold it again as it crosses out.
                if let Error::Exception(exc) = err {
                    mrb.hold(exc);
                }
                Err(err)
            }
            None => result.map(|_| ()),
        }
    }
}

/// Run `visit` on one pair, a panic in it surfacing as an `Err`.
fn visit_one<F>(visit: &mut F, key: Value, val: Value) -> Result<ForEach, Error>
where
    F: FnMut(Value, Value) -> Result<ForEach, Error>,
{
    crate::sys::catch_unwind(std::panic::AssertUnwindSafe(|| visit(key, val))).and_then(|res| res)
}

crate::value::value_backed_repr!(RHash);

impl FromValue for RHash {
    #[inline]
    fn from_value(value: Value) -> Option<Self> {
        // SAFETY: the wrap precondition (MRB_TT_HASH tagging) is
        // established by the tag check immediately before it.
        (value.tag() == sys::MRB_TT_HASH).then(|| unsafe { RHash::from_value_unchecked(value) })
    }
}

crate::try_convert::try_convert_tagged!(RHash => "Hash");
