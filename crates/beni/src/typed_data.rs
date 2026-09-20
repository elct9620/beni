//! Rust data carried by mruby objects — beni's mirror of magnus's
//! `typed_data`.
//!
//! A Rust type implements `TypedData` to name its `DataType` and the
//! class its values wrap as; `Mrb::wrap` / `Mrb::obj_wrap` box a value
//! into a data carrier (`CDATA`) of that class, answered as an untyped
//! `RTypedData` or a typed `Obj<T>` handle, and `&T` / `Obj<T>` convert
//! back through `TryConvert`. The mruby GC owns a wrapped payload and
//! drops it through the data type's release hook; nothing on the typed
//! surface replaces a payload once a carrier holds one.

use crate::{
    sys::AsRawValue, DataType, Error, IntoValue, Mrb, RClass, ReprValue, TryConvert, Value,
};
use beni_sys as sys;
use core::marker::PhantomData;
use core::ops::Deref;

/// A Rust type mruby objects carry as their payload. Mirrors magnus's
/// `TypedData`.
///
/// The payload travels with the interpreter and is dropped by the
/// release hook on whichever thread reaches it, so only a `Send` type
/// is `TypedData`:
///
/// ```
/// # use beni::{DataType, Mrb, RClass, TypedData};
/// struct Counter(u32);
/// static COUNTER: DataType<Counter> = DataType::new(c"Counter");
/// unsafe impl TypedData for Counter {
///     fn class(mrb: &Mrb) -> RClass { mrb.object_class() }
///     fn data_type() -> &'static DataType<Self> { &COUNTER }
/// }
/// ```
///
/// ```compile_fail
/// # use beni::{DataType, Mrb, RClass, TypedData};
/// struct Bare(*const ());
/// static BARE: DataType<Bare> = DataType::new(c"Bare");
/// unsafe impl TypedData for Bare {
///     fn class(mrb: &Mrb) -> RClass { mrb.object_class() }
///     fn data_type() -> &'static DataType<Self> { &BARE }
/// }
/// ```
///
/// # Safety
///
/// Every class `class` and `class_for` name must be marked through
/// `RClass::set_instance_data_tt` before a value wraps into it, so a
/// wrap allocates a data carrier rather than raising; `mark_carriers`
/// is where an implementation does that marking. `class_for` names
/// `class` or a subclass of it.
pub unsafe trait TypedData: Send + Sized + 'static {
    /// The class a value of this type wraps as, and the class a
    /// conversion failure names.
    fn class(mrb: &Mrb) -> RClass;

    /// The data type this Rust type's carriers are tagged with.
    fn data_type() -> &'static DataType<Self>;

    /// The class one particular value wraps as — `class` unless
    /// overridden.
    fn class_for(mrb: &Mrb, value: &Self) -> RClass {
        let _ = value;
        Self::class(mrb)
    }

    /// Prepare every class this type names in `mrb` to carry Rust
    /// data: marked as a carrier, its default allocator undefined.
    /// This is how an implementation keeps the trait's contract, and
    /// the embedder calls it once per interpreter while its gems
    /// install, before any Ruby program runs. Preparing the class
    /// `class` answers is what an implementation written by hand
    /// needs; the `wrap` and `TypedData` macros prepare each class
    /// they name and hold it in the interpreter's carrier record.
    fn mark_carriers(mrb: &Mrb) -> Result<(), Error> {
        let class = Self::class(mrb);
        class.set_instance_data_tt(mrb)?;
        class.undef_default_alloc_func(mrb);
        Ok(())
    }
}

/// Untyped handle on a data carrier — an mruby object carrying Rust
/// data of any data type, or a bare carrier holding none. Mirrors
/// magnus's `RTypedData`.
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct RTypedData(Value);

impl RTypedData {
    /// Wrap a `Value` the caller has already determined to be a data
    /// carrier.
    ///
    /// # Safety
    ///
    /// `v` must be `CDATA`-tagged.
    #[inline]
    pub unsafe fn from_value_unchecked(v: Value) -> Self {
        Self(v)
    }

    /// Borrow the payload as a `T`, or answer the `TypeError` mruby's
    /// data-type check raises when the carrier holds no payload or one
    /// of another data type.
    #[inline]
    pub fn get<T: TypedData>(self, mrb: &Mrb) -> Result<&T, Error> {
        // SAFETY: the borrow is bounded by `mrb`, and the payload lives
        // while its carrier stays reachable (the GC validity rule).
        payload(self.0, mrb).map(|ptr| unsafe { &*ptr })
    }
}

/// Typed handle on a data carrier known to hold a `T`. Dereferences to
/// the payload. Mirrors magnus's `typed_data::Obj`.
#[repr(transparent)]
pub struct Obj<T> {
    inner: RTypedData,
    _marker: PhantomData<T>,
}

impl<T: TypedData> Copy for Obj<T> {}

impl<T: TypedData> Clone for Obj<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: TypedData> Deref for Obj<T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: an `Obj<T>` is made only by wrapping a `T` or by a
        // conversion that checked the carrier holds `T`'s data type, and
        // no typed operation replaces a held payload, so the carrier's
        // data slot points at a live `T` while it stays reachable.
        unsafe {
            let rdata = sys::mrb_obj_ptr_func(self.inner.0.as_raw()) as *const sys::RData;
            &*((*rdata).data as *const T)
        }
    }
}

impl<T: TypedData> crate::value::private::ReprValue for Obj<T> {
    #[inline]
    unsafe fn from_value_unchecked(v: Value) -> Self {
        Self {
            inner: RTypedData(v),
            _marker: PhantomData,
        }
    }
}

impl<T: TypedData> ReprValue for Obj<T> {
    #[inline]
    fn as_value(self) -> Value {
        self.inner.0
    }
}

impl Mrb {
    /// Wrap `data` as a new instance of the class its type names for
    /// it. Mirrors magnus's `Ruby::wrap`.
    #[inline]
    pub fn wrap<T: TypedData>(&self, data: T) -> RTypedData {
        let class = T::class_for(self, &data);
        self.wrap_as(data, class)
    }

    /// Wrap `data` as a new instance of `class` — `T`'s class or a
    /// subclass of it. Mirrors magnus's `Ruby::wrap_as`.
    ///
    /// # Panics
    ///
    /// When `class` cannot carry data — it was never marked, breaking
    /// `TypedData`'s contract. The payload is dropped first.
    pub fn wrap_as<T: TypedData>(&self, data: T, class: RClass) -> RTypedData {
        let ptr = Box::into_raw(Box::new(data)) as *mut core::ffi::c_void;
        // `ptr` is `Copy`, so the closure captures a copy while this
        // frame keeps the original for the reclaim path.
        let wrapped = self.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame; `class`
            // is from the same VM; `ptr` is a freshly leaked `Box<T>`;
            // the data type is `'static`, so it outlives the carrier.
            // Allocating against an unmarked class raises a
            // `TypeError`, caught by `protect`.
            let rdata = unsafe {
                sys::mrb_data_object_alloc(
                    mrb.as_ptr(),
                    class.as_internal(),
                    ptr,
                    T::data_type().as_raw(),
                )
            };
            // SAFETY: `rdata` is a live object just allocated.
            Value::from_raw_unchecked(unsafe { sys::mrb_obj_value(rdata as *mut _) })
        });
        match wrapped {
            Ok(value) => {
                debug_assert!(
                    value.is_kind_of(self, T::class(self)),
                    "{} is not a subclass of {}",
                    class.as_value().inspect(self),
                    T::class(self).as_value().inspect(self),
                );
                RTypedData(value)
            }
            Err(err) => {
                // SAFETY: the allocation raised before any carrier took
                // the box, so this frame still owns it.
                drop(unsafe { Box::from_raw(ptr as *mut T) });
                panic!("a TypedData class cannot carry data: {}", err.message(self));
            }
        }
    }

    /// As `wrap`, answered as the typed `Obj<T>`. Mirrors magnus's
    /// `Ruby::obj_wrap`.
    #[inline]
    pub fn obj_wrap<T: TypedData>(&self, data: T) -> Obj<T> {
        typed(self.wrap(data))
    }

    /// As `wrap_as`, answered as the typed `Obj<T>`. Mirrors magnus's
    /// `Ruby::obj_wrap_as`.
    #[inline]
    pub fn obj_wrap_as<T: TypedData>(&self, data: T, class: RClass) -> Obj<T> {
        typed(self.wrap_as(data, class))
    }
}

/// Copies a `TypedData` payload across mruby's `dup` and `clone`, which
/// otherwise copy a carrier without it. Mirrors magnus's
/// `typed_data::Dup`; register each as the method it replaces:
///
/// ```ignore
/// class.define_method(mrb, c"dup", method!(<Point as Dup>::dup, 0))?;
/// class.define_method(mrb, c"clone", method!(<Point as Dup>::clone, -1))?;
/// ```
pub trait Dup: Sized {
    /// A clone of the receiver's payload, which the method's return
    /// wraps as a new instance.
    fn dup(mrb: &Mrb, rb_self: &Self) -> Self;

    /// Copy the receiver as mruby's `clone` does — singleton class and
    /// frozen state kept, `initialize_copy` run — carrying a clone of
    /// its payload. Takes no arguments, as mruby's `clone` does not.
    fn clone(mrb: &Mrb, rb_self: Obj<Self>) -> Result<Obj<Self>, Error>;
}

impl<T: Clone + TypedData> Dup for T {
    fn dup(_mrb: &Mrb, rb_self: &Self) -> Self {
        rb_self.clone()
    }

    fn clone(mrb: &Mrb, rb_self: Obj<Self>) -> Result<Obj<Self>, Error> {
        crate::scan_args::scan_args::<(), (), (), (), (), ()>(mrb)?;
        let copy = rb_self.as_value().obj_clone(mrb)?;
        let payload = Box::into_raw(Box::new((*rb_self).clone()));
        // SAFETY: `copy` is the carrier `mrb_obj_clone` just made, which
        // holds no payload; the box is handed to it under `T`'s data
        // type, whose release hook drops it with the carrier.
        unsafe {
            sys::mrb_data_init(copy.as_raw(), payload.cast(), T::data_type().as_raw());
        }
        Ok(typed(RTypedData(copy)))
    }
}

fn typed<T: TypedData>(inner: RTypedData) -> Obj<T> {
    Obj {
        inner,
        _marker: PhantomData,
    }
}

/// The payload of `val` as a `T`, or the `TypeError` mruby's own
/// `mrb_data_check_type` raises for a value that is no carrier, a
/// carrier of another data type, or a bare carrier.
fn payload<T: TypedData>(val: Value, mrb: &Mrb) -> Result<*const T, Error> {
    let ty = T::data_type().as_raw();
    // SAFETY: `mrb` is alive and `val` is from the same VM;
    // `mrb_data_check_get_ptr` answers NULL unless `val` carries `ty`.
    let ptr = unsafe { sys::mrb_data_check_get_ptr(mrb.as_ptr(), val.as_raw(), ty) };
    if !ptr.is_null() {
        return Ok(ptr as *const T);
    }
    mrb.protect(|mrb| {
        // SAFETY: `mrb` is alive inside the protect frame; the check
        // raises the mismatch's `TypeError`, caught by `protect`.
        unsafe { sys::mrb_data_check_type(mrb.as_ptr(), val.as_raw(), ty) };
        Value::nil()
    })?;
    // A carrier of `T`'s data type whose payload is NULL is a bare
    // carrier the check lets through; it is uninitialized all the same.
    Err(crate::try_convert::type_error(
        mrb,
        &format!(
            "uninitialized {} (expected {})",
            val.classname(mrb),
            T::data_type().name()
        ),
    ))
}

impl TryConvert for RTypedData {
    fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
        if val.is_data() {
            return Ok(RTypedData(val));
        }
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame; a
            // non-carrier raises mruby's own `TypeError`, caught by
            // `protect`.
            unsafe { sys::mrb_check_type(mrb.as_ptr(), val.as_raw(), sys::MRB_TT_CDATA) };
            Value::nil()
        })?;
        unreachable!("mrb_check_type raises for every value that is no data carrier")
    }
}

impl<T: TypedData> TryConvert for &T {
    fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
        // SAFETY: the payload lives while its carrier stays reachable,
        // which the consumer upholds under the GC validity rule, as for
        // magnus's unconstrained typed-data reference.
        payload(val, mrb).map(|ptr| unsafe { &*ptr })
    }
}

impl<T: TypedData> TryConvert for Obj<T> {
    fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
        payload::<T>(val, mrb)?;
        Ok(typed(RTypedData(val)))
    }
}

impl IntoValue for RTypedData {
    #[inline]
    fn into_value(self, _mrb: &Mrb) -> Value {
        self.0
    }
}

impl<T: TypedData> IntoValue for Obj<T> {
    #[inline]
    fn into_value(self, _mrb: &Mrb) -> Value {
        self.inner.0
    }
}

impl<T: TypedData> IntoValue for T {
    #[inline]
    fn into_value(self, mrb: &Mrb) -> Value {
        mrb.wrap(self).0
    }
}

impl ReprValue for RTypedData {
    #[inline]
    fn as_value(self) -> Value {
        self.0
    }
}

impl crate::value::private::ReprValue for RTypedData {
    #[inline]
    unsafe fn from_value_unchecked(v: Value) -> Self {
        Self(v)
    }
}
