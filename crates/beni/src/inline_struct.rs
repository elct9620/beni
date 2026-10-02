//! Plain Rust data stored inside mruby objects — mruby's inline structs
//! (`ISTRUCT`).
//!
//! A Rust type implements `InlineStruct` to name the class its values
//! wrap as and the `InlineType` diagnostics name it by; `Inline::new`
//! stores a value inside a new instance of that class and `Inline<T>`
//! reads and replaces it by copy. The object carries no type tag of its
//! own, so the interpreter's carrier record names the type each marked
//! class belongs to.

use crate::{sys::AsRawValue, Error, IntoValue, Mrb, RClass, ReprValue, TryConvert, Value};
use beni_sys as sys;
use core::ffi::CStr;
use core::marker::PhantomData;

/// A plain-data Rust type mruby objects store inside themselves.
///
/// `bytemuck::Pod` keeps the payload free of values and drop glue: the
/// collector never traces it and mruby never releases it. The payload
/// fits three pointer widths at pointer alignment, or the type does not
/// compile as an `InlineStruct`:
///
/// ```
/// # use beni::{InlineStruct, InlineType, Mrb, RClass};
/// #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
/// #[repr(C)]
/// struct Vector2D { x: f64, y: f64 }
/// static VECTOR2D: InlineType<Vector2D> = InlineType::new(c"Vector2D");
/// unsafe impl InlineStruct for Vector2D {
///     fn class(mrb: &Mrb) -> RClass { mrb.object_class() }
///     fn inline_type() -> &'static InlineType<Self> { &VECTOR2D }
/// }
/// ```
///
/// A payload past three pointer widths does not compile:
///
/// ```compile_fail
/// # use beni::{Inline, InlineStruct, InlineType, Mrb, RClass};
/// #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
/// #[repr(C)]
/// struct Wide([usize; 4]);
/// static WIDE: InlineType<Wide> = InlineType::new(c"Wide");
/// unsafe impl InlineStruct for Wide {
///     fn class(mrb: &Mrb) -> RClass { mrb.object_class() }
///     fn inline_type() -> &'static InlineType<Self> { &WIDE }
/// }
/// let mrb = Mrb::open().unwrap();
/// Inline::new(&mrb, Wide([0; 4]));
/// ```
///
/// Nor does one holding a value, which `Pod` refuses:
///
/// ```compile_fail
/// # use beni::{InlineStruct, InlineType, Mrb, RClass, Value};
/// #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
/// #[repr(C)]
/// struct Holder(Value);
/// static HOLDER: InlineType<Holder> = InlineType::new(c"Holder");
/// unsafe impl InlineStruct for Holder {
///     fn class(mrb: &Mrb) -> RClass { mrb.object_class() }
///     fn inline_type() -> &'static InlineType<Self> { &HOLDER }
/// }
/// ```
///
/// # Safety
///
/// The class `class` names must be marked for this type through
/// `RClass::set_instance_inline_tt` before a value wraps into it;
/// `mark_carriers` is where an implementation does that marking.
pub unsafe trait InlineStruct: bytemuck::Pod + Send {
    /// The class a value of this type wraps as.
    fn class(mrb: &Mrb) -> RClass;

    /// The descriptor naming this type, whose address is the type's
    /// identity in the carrier record.
    fn inline_type() -> &'static InlineType<Self>;

    /// Prepare the class this type names in `mrb`: marked so its
    /// instances are inline structs of this type, its default
    /// allocator undefined. The embedder calls it once per interpreter
    /// while its gems install, before any Ruby program runs.
    fn mark_carriers(mrb: &Mrb) -> Result<(), Error> {
        let class = Self::class(mrb);
        class.set_instance_inline_tt::<Self>(mrb)?;
        class.undef_default_alloc_func(mrb);
        Ok(())
    }
}

/// The descriptor an `InlineStruct` type names itself by. Declared as a
/// `static`, whose address identifies the type.
pub struct InlineType<T> {
    name: &'static CStr,
    _marker: PhantomData<fn() -> T>,
}

impl<T> InlineType<T> {
    /// A descriptor whose `name` mruby diagnostics show for the type.
    pub const fn new(name: &'static CStr) -> Self {
        Self {
            name,
            _marker: PhantomData,
        }
    }

    fn tag(&'static self) -> *const () {
        (self as *const Self).cast()
    }

    fn name(&self) -> std::borrow::Cow<'static, str> {
        self.name.to_string_lossy()
    }
}

/// Typed handle on an inline struct of `T`. Mirrors `Obj<T>`, reading
/// and replacing the payload by copy where `Obj<T>` borrows it.
#[repr(transparent)]
pub struct Inline<T> {
    value: Value,
    _marker: PhantomData<T>,
}

impl<T: InlineStruct> Copy for Inline<T> {}

impl<T: InlineStruct> Clone for Inline<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: InlineStruct> Inline<T> {
    /// Store `data` inside a new instance of the class `T` names.
    ///
    /// # Panics
    ///
    /// When that class does not belong to `T` — it was never marked for
    /// it, breaking `InlineStruct`'s contract.
    pub fn new(mrb: &Mrb, data: T) -> Self {
        let () = Fits::<T>::INSIDE;
        let class = T::class(mrb);
        assert!(
            belongs_to::<T>(mrb, class),
            "an InlineStruct class does not belong to {}",
            T::inline_type().name()
        );
        let value = mrb
            .protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame and
                // `class` allocates inline structs; only exhausting
                // memory raises, caught by `protect`.
                let object = unsafe {
                    sys::mrb_obj_alloc(mrb.as_ptr(), sys::MRB_TT_ISTRUCT, class.as_internal())
                };
                // SAFETY: `object` is a live object just allocated.
                Value::from_raw_unchecked(unsafe { sys::mrb_obj_value(object.cast()) })
            })
            .unwrap_or_else(|err| {
                panic!("allocating an inline struct raised: {}", err.message(mrb))
            });
        let inline = Self {
            value,
            _marker: PhantomData,
        };
        inline.write(data);
        inline
    }

    /// A copy of the payload.
    pub fn get(self) -> T {
        let () = Fits::<T>::INSIDE;
        // SAFETY: an `Inline<T>` names an inline struct of `T`, whose
        // payload holds a `T` at pointer alignment.
        unsafe { core::ptr::read(sys::mrb_istruct_ptr(self.value.as_raw()) as *const T) }
    }

    /// Replace the payload with `data`, or answer the `FrozenError`
    /// mruby raises for a frozen receiver, leaving the payload as it was.
    pub fn set(self, mrb: &Mrb, data: T) -> Result<(), Error> {
        self.value.check_frozen(mrb)?;
        self.write(data);
        Ok(())
    }

    fn write(self, data: T) {
        let () = Fits::<T>::INSIDE;
        // SAFETY: as `get`; `Pod` holds no value, so the write needs no
        // write barrier.
        unsafe { core::ptr::write(sys::mrb_istruct_ptr(self.value.as_raw()) as *mut T, data) }
    }
}

impl RClass {
    /// Mark this class, and any class later defined from it, so its
    /// instances are inline structs of `T`. Only a class whose instances
    /// are plain objects, or a class already belonging to `T`, accepts;
    /// any other refuses with a `TypeError` and stays unmarked.
    pub fn set_instance_inline_tt<T: InlineStruct>(self, mrb: &Mrb) -> Result<(), Error> {
        let tt = crate::class::instance_tt(self.as_internal());
        let plain = self.as_value().is_class() && tt == sys::MRB_TT_OBJECT;
        if !plain && !(tt == sys::MRB_TT_ISTRUCT && belongs_to::<T>(mrb, self)) {
            return Err(crate::try_convert::type_error(
                mrb,
                &format!(
                    "can't mark a class to carry {} unless its instances are plain objects or inline structs of it",
                    T::inline_type().name()
                ),
            ));
        }
        mrb.hold_inline(self, T::inline_type().tag())?;
        // SAFETY: `self` is a live class of the VM borrowed as `mrb`; the
        // shim only rewrites its instance-type flag bits.
        unsafe { sys::mrb_set_instance_tt_func(self.as_internal(), sys::MRB_TT_ISTRUCT) };
        Ok(())
    }
}

fn belongs_to<T: InlineStruct>(mrb: &Mrb, class: RClass) -> bool {
    mrb.inline_owner(class) == Some(T::inline_type().tag())
}

/// Holds when `T` fits mruby's inline-struct payload; evaluated wherever
/// a payload is read or written, so a type too large does not compile.
struct Fits<T>(PhantomData<T>);

impl<T> Fits<T> {
    const INSIDE: () = assert!(
        core::mem::size_of::<T>() <= 3 * core::mem::size_of::<*const ()>()
            && core::mem::align_of::<T>() <= core::mem::align_of::<*const ()>(),
        "an InlineStruct payload fits three pointer widths at pointer alignment"
    );
}

impl<T: InlineStruct> TryConvert for Inline<T> {
    fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
        // SAFETY: `mrb_type` is a pure predicate over the value tag.
        let istruct = unsafe { sys::mrb_type(val.as_raw()) } == sys::MRB_TT_ISTRUCT;
        if istruct && belongs_to::<T>(mrb, val.class(mrb)) {
            return Ok(Self {
                value: val,
                _marker: PhantomData,
            });
        }
        Err(crate::try_convert::wrong_argument_type(
            val,
            mrb,
            &T::inline_type().name(),
        ))
    }
}

impl<T: InlineStruct> IntoValue for Inline<T> {
    #[inline]
    fn into_value(self, _mrb: &Mrb) -> Value {
        self.value
    }
}

impl<T: InlineStruct> ReprValue for Inline<T> {
    #[inline]
    fn as_value(self) -> Value {
        self.value
    }
}

impl<T: InlineStruct> crate::value::private::ReprValue for Inline<T> {
    #[inline]
    unsafe fn from_value_unchecked(v: Value) -> Self {
        Self {
            value: v,
            _marker: PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_payload_bound_is_the_size_mruby_reports() {
        // SAFETY: `mrb_istruct_size` reads a compile-time constant.
        let reported = unsafe { beni_sys::mrb_istruct_size() };
        assert_eq!(
            usize::try_from(reported).ok(),
            Some(3 * core::mem::size_of::<*const ()>())
        );
    }
}
