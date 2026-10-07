//! The `Object` trait, the per-object surface of an instance-variable
//! holder — beni's mirror of magnus's `Object`.

use crate::class::protect_register;
use crate::{
    Error, ExceptionClass, FromValue, IntoId, IntoValue, MethodDef, Mrb, Qundef, RClass, RModule,
    ReprValue, TryConvert, Value,
};
use beni_sys as sys;

/// Per-object surface of an instance-variable holder — beni's mirror of
/// `magnus::Object`: singleton methods, instance variables, and the
/// singleton class. Implemented only by the handles whose object mruby
/// lets hold instance variables (`obj_iv_p`), which is narrower than
/// CRuby's set: a string, array, or other built-in layout carries none.
///
/// A handle outside that set does not implement the trait:
///
/// ```compile_fail
/// fn holder<T: beni::Object>() {}
/// holder::<beni::RString>();
/// ```
///
/// ```compile_fail
/// fn holder<T: beni::Object>() {}
/// holder::<beni::Value>();
/// ```
pub trait Object: ReprValue {
    /// `mrb_define_singleton_method_id(mrb, self, name, func, aspec)` —
    /// register a method on this object's singleton class from a
    /// `method!`-wrapped Rust function. The name is a symbol-or-name
    /// key (`IntoId`). A singleton method on a class is its class method.
    fn define_singleton_method<K: IntoId>(
        self,
        mrb: &Mrb,
        name: K,
        method: MethodDef,
    ) -> Result<(), Error> {
        let sym = name.into_id(mrb)?.to_raw();
        protect_register(mrb, method, |mrb, raw, aspec| {
            // SAFETY: as `Module::define_method`; every holder is a heap
            // object, so `mrb_obj_ptr` reads its `RObject *` header,
            // which mruby's singleton-class preparation accepts.
            unsafe {
                sys::mrb_define_singleton_method_id(
                    mrb.as_ptr(),
                    sys::mrb_obj_ptr_func(self.as_value().0),
                    sym,
                    raw,
                    aspec,
                )
            };
        })
    }

    /// Undefine a singleton method on this object, through its singleton
    /// class: the per-object counterpart of `Module::undef_method`. The
    /// name is a symbol-or-name key (`IntoId`); undefining a name absent
    /// from the object surfaces as `Err(Error::Exception)` (mruby's
    /// `NameError`).
    fn undef_singleton_method<K: IntoId>(self, mrb: &Mrb, name: K) -> Result<(), Error> {
        let sym = name.into_id(mrb)?.to_raw();
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame; `self`
            // originates from the same VM; `sym` was interned against
            // it. `mrb_singleton_class` yields a class-tagged value or
            // raises, and `mrb_undef_method_id` raises NameError when
            // the method is absent — both caught by `protect`.
            unsafe {
                let singleton = sys::mrb_singleton_class(mrb.as_ptr(), self.as_value().0);
                sys::mrb_undef_method_id(mrb.as_ptr(), sys::mrb_class_ptr_func(singleton), sym);
            }
            crate::value::qnil()
        })
        .map(|_| ())
    }

    /// `mrb_iv_get(mrb, self, sym)` — read instance variable `name` from
    /// `self` through `TryConvert` into `U`; an unset variable reads as
    /// `nil`. Surfaces only the key's or the conversion's `Err`.
    #[inline]
    fn ivar_get<K: IntoId, U: TryConvert>(self, mrb: &Mrb, name: K) -> Result<U, Error> {
        let sym = name.into_id(mrb)?.to_raw();
        // SAFETY: `mrb` is alive; `self` is a holder of the same VM, and
        // `mrb_iv_get` reads its table without dispatching.
        let value = mrb.hold(Value(unsafe {
            sys::mrb_iv_get(mrb.as_ptr(), self.as_value().0, sym)
        }));
        U::try_convert(value, mrb)
    }

    /// `mrb_iv_set(mrb, self, sym, val)` — assign instance variable
    /// `name` on `self` to `val`. Surfaces an `Err` when `self` is frozen.
    #[inline]
    fn ivar_set<K: IntoId, U: IntoValue>(self, mrb: &Mrb, name: K, val: U) -> Result<(), Error> {
        let sym = name.into_id(mrb)?.to_raw();
        let val = val.into_value(mrb);
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame; `self` and
            // `val` originate from the same VM. `mrb_iv_set` raises
            // `FrozenError` on a frozen holder — caught by `protect`.
            unsafe { sys::mrb_iv_set(mrb.as_ptr(), self.as_value().0, sym, val.0) };
            crate::value::qnil()
        })
        .map(|_| ())
    }

    /// `mrb_iv_defined(mrb, self, sym)` — TRUE when instance variable
    /// `name` is set on `self`. A name too long to be a symbol answers
    /// false; `mrb_obj_iv_defined`, the raw-`RObject*` form, stays in
    /// `sys`.
    #[inline]
    fn ivar_defined<K: IntoId>(self, mrb: &Mrb, name: K) -> bool {
        let Ok(sym) = name.into_id(mrb).map(crate::Id::to_raw) else {
            return false;
        };
        // SAFETY: `mrb` is alive; `self` is a holder of the same VM.
        unsafe { sys::mrb_iv_defined(mrb.as_ptr(), self.as_value().0, sym) }
    }

    /// `mrb_iv_remove(mrb, self, sym)` — remove instance variable `name`
    /// from `self`, returning `Some` of its former value, or `None` when
    /// it is absent — distinct from a variable removed while holding
    /// `nil`. Surfaces an `Err` only when `self` is frozen.
    #[inline]
    fn ivar_remove<K: IntoId>(self, mrb: &Mrb, name: K) -> Result<Option<Value>, Error> {
        let sym = name.into_id(mrb)?.to_raw();
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame; `self`
            // originates from the same VM. `mrb_iv_remove` raises
            // `FrozenError` on a frozen holder — caught by `protect`.
            Value(unsafe { sys::mrb_iv_remove(mrb.as_ptr(), self.as_value().0, sym) })
        })
        .map(|removed| Qundef::from_value(removed).is_none().then_some(removed))
    }

    /// `mrb_iv_foreach(mrb, self, …)` — visit each instance variable set
    /// on `self` in iv-table order, handing its name as a typed `Symbol`
    /// and its value to `body`. Returning `ForEach::Stop` ends the
    /// iteration before the remaining variables; `ForEach::Continue`
    /// proceeds. The iteration visits the variables and the values they
    /// held when it began: `body` reassigning, removing, or adding the
    /// holder's instance variables changes the holder but never the
    /// visited set, and each visited value holds arena protection as if
    /// created here, staying valid across `body`'s own mutations and
    /// collections. The iteration dispatches no Ruby and so never raises;
    /// magnus binds no ivar foreach, so this anchors on mruby's own
    /// `mrb_iv_foreach`.
    ///
    /// A panic in `body` ends the iteration and propagates; `body` runs
    /// after the C walk has finished, so the panic never crosses
    /// mruby's frames.
    #[inline]
    fn ivar_foreach<F>(self, mrb: &Mrb, body: F)
    where
        F: FnMut(crate::Symbol, Value) -> crate::ForEach,
    {
        // Snapshot the (name, value) pairs before any caller code
        // runs: the C foreach walks the live iv table, which `body`
        // re-entering the VM could free and reallocate mid-walk, so
        // `body` only ever runs against this collected copy. Each
        // value is arena-protected as it is collected — the holder
        // stops referencing a value `body` removes, and the snapshot
        // must outlive any collection `body` triggers.
        unsafe extern "C" fn collect(
            mrb: *mut sys::mrb_state,
            name: sys::mrb_sym,
            val: sys::mrb_value,
            data: *mut core::ffi::c_void,
        ) -> core::ffi::c_int {
            // SAFETY: `data` is the `&mut Vec<…>` handed to
            // `mrb_iv_foreach` below, borrowed for the duration of
            // the walk on this same thread; `mrb` is the live state
            // driving the walk, and protecting into the arena leaves
            // the iv table untouched.
            let pairs: &mut Vec<(crate::Symbol, Value)> =
                unsafe { &mut *(data as *mut Vec<(crate::Symbol, Value)>) };
            unsafe { sys::mrb_gc_protect(mrb, val) };
            pairs.push((
                crate::Symbol::from(crate::Id::from_raw_unchecked(name)),
                Value::from_raw_unchecked(val),
            ));
            0
        }

        let mut pairs: Vec<(crate::Symbol, Value)> = Vec::new();
        // SAFETY: `mrb` is alive; `self` originates from the same VM.
        // `collect` upholds the `mrb_iv_foreach_func` ABI and runs no
        // caller code; `data` points to `pairs` on this frame, which
        // outlives the call. bindgen wraps the function-typedef
        // parameter in `Option`, so the collector is passed via `Some`.
        unsafe {
            sys::mrb_iv_foreach(
                mrb.as_ptr(),
                self.as_value().0,
                Some(collect),
                &mut pairs as *mut Vec<(crate::Symbol, Value)> as *mut core::ffi::c_void,
            );
        }
        let mut body = body;
        for (name, val) in pairs {
            if let crate::ForEach::Stop = body(name, val) {
                break;
            }
        }
    }

    /// `mrb_singleton_class(mrb, self)` — the holder's own singleton
    /// class, Ruby's `singleton_class`: the per-instance eigenclass that
    /// holds methods defined on that one object, distinct from the
    /// regular class `ReprValue::class` returns. Created on first read
    /// and stable across re-reads. Surfaces an `Err` carrying a
    /// `TypeError` only where mruby gives the object no singleton class.
    /// The raw `RClass*` form (`mrb_singleton_class_ptr`) stays behind
    /// `beni::sys`.
    #[inline]
    fn singleton_class(self, mrb: &Mrb) -> Result<RClass, Error> {
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame; `self`
            // originates from the same VM. `mrb_singleton_class` raises
            // `TypeError` when the object has no singleton class —
            // caught by `protect` — and otherwise returns a
            // class-tagged value.
            let v = Value::from_raw_unchecked(unsafe {
                sys::mrb_singleton_class(mrb.as_ptr(), self.as_value().0)
            });
            // SAFETY: a value returned without a raise is class-tagged,
            // so the pointer recovery accepts it.
            RClass::from_raw_unchecked(unsafe { v.as_class_ptr() })
        })
    }
}

impl Object for RClass {}
impl Object for RModule {}
impl Object for ExceptionClass {}
impl Object for crate::RObject {}
impl Object for crate::RHash {}
impl Object for crate::RTypedData {}
impl<T: crate::TypedData> Object for crate::typed_data::Obj<T> {}
impl Object for crate::Exception {}
