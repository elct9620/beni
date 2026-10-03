//! Public-surface drift net, held from consumer position.
//!
//! Every inherent `pub fn` the `beni` crate declares is named here
//! once, through the paths a consumer reaches it by, and every macro
//! it re-exports is applied once. An item that stops being public — or
//! a root re-export its path runs through that stops being exported —
//! breaks this file's compilation, which no in-crate test can see. `rake api:surface` keeps the list and
//! the crate's own surface from drifting apart in either direction.

use beni::*;

#[test]
fn full_api_surface_is_reachable_from_outside() {
    let _ = Mrb::open;
    let _ = Mrb::as_ptr;
    let _ = Mrb::borrow_raw;
    let _ = Mrb::pending_exc;
    let _ = Mrb::set_pending_exc;
    let _ = Mrb::clear_exc;
    let _ = Mrb::object_class;
    let _ = Mrb::define_module::<&core::ffi::CStr>;
    let _ = Mrb::define_class::<&core::ffi::CStr>;
    let _ = Mrb::define_error::<&core::ffi::CStr>;
    let _ = Mrb::class_get::<&core::ffi::CStr>;
    let _ = Mrb::module_get::<&core::ffi::CStr>;
    let _ = Mrb::define_global_const;
    let _ = Mrb::gv_set::<&core::ffi::CStr>;
    let _ = Mrb::gv_get::<&core::ffi::CStr>;
    let _ = Mrb::arena_scope;
    let _ = ArenaScope::keep;
    let _ = Mrb::str_new;
    let _ = Mrb::str_new_cstr;
    let _ = Mrb::str_new_capa;
    let _ = Mrb::ary_new;
    let _ = Mrb::hash_new;
    let _ = Mrb::range_new;
    let _ = Mrb::intern_cstr;
    let _ = Mrb::intern_str;
    let _ = Mrb::sym_name;
    let _ = Mrb::load_bytecode;
    let _ = scan_args::scan_args::<(), (), (), (), (), ()>;
    let _ = scan_args::get_kwargs::<&str, (), (), ()>;
    let _ = Value::nil;
    let _ = Value::true_;
    let _ = Value::false_;
    let _ = <Value as ReprValue>::to_r_string;
    let _ = <Value as ReprValue>::any_to_s;
    let _ = <Value as ReprValue>::dup;
    let _ = <Value as ReprValue>::classname;
    let _ = <Value as ReprValue>::to_string;
    let _ = <Value as ReprValue>::funcall::<&core::ffi::CStr>;
    let _ = <Value as ReprValue>::is_nil;
    let _ = <RObject as Object>::ivar_set::<&core::ffi::CStr, Value>;
    let _ = <RObject as Object>::ivar_get::<&core::ffi::CStr, Value>;
    let _ = <RObject as Object>::ivar_defined::<&core::ffi::CStr>;
    let _ = <RClass as Module>::const_defined::<&core::ffi::CStr>;
    let _ = <RClass as Module>::const_get::<&core::ffi::CStr, Value>;
    let _ = <RClass as Module>::cvar_get::<&core::ffi::CStr, Value>;
    let _ = <Value as ReprValue>::respond_to::<&core::ffi::CStr>;
    let _ = <Value as ReprValue>::as_break;
    let _ = Break::value;
    let _ = RClass::is_singleton;
    let _ = RClass::real;
    let _ = RClass::new_instance;
    let _ = RClass::set_instance_data_tt;
    let _ = RClass::undef_default_alloc_func;
    {
        struct Probe;
        static PROBE: DataType<Probe> = DataType::new(c"Probe");
        // SAFETY: never wrapped; named only for its paths.
        unsafe impl TypedData for Probe {
            fn class(mrb: &Mrb) -> RClass {
                mrb.object_class()
            }
            fn data_type() -> &'static DataType<Self> {
                &PROBE
            }
        }
        let _ = Mrb::wrap::<Probe>;
        let _ = Mrb::wrap_as::<Probe>;
        let _ = Mrb::obj_wrap::<Probe>;
        let _ = Mrb::obj_wrap_as::<Probe>;
        let _ = RTypedData::from_value_unchecked;
        let _ = RTypedData::get::<Probe>;
    }
    {
        #[beni::wrap(class = "Wrapped")]
        struct Wrapped;
        #[derive(beni::TypedData)]
        #[beni(class = "Derived")]
        struct Derived;
        let _ = Mrb::wrap::<Wrapped>;
        let _ = Mrb::wrap::<Derived>;
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, beni::InlineStruct)]
        #[beni(class = "Inlined")]
        #[repr(C)]
        struct Inlined(u32);
        let _ = Inline::<Inlined>::new;
        let _ = Mrb::mark_inline_carrier::<Inlined>;
    }
    let _ = DataType::<i32>::new;
    {
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        #[repr(C)]
        struct Packed(u32);
        static PACKED: InlineType<Packed> = InlineType::new(c"Packed");
        // SAFETY: never wrapped; named only for its paths.
        unsafe impl InlineStruct for Packed {
            fn class(mrb: &Mrb) -> RClass {
                mrb.object_class()
            }
            fn inline_type() -> &'static InlineType<Self> {
                &PACKED
            }
        }
        let _ = Inline::<Packed>::new;
        let _ = Inline::<Packed>::get;
        let _ = Inline::<Packed>::set;
        let _ = RClass::set_instance_inline_tt::<Packed>;
    }
    let _ = InlineType::<i32>::new;
    let _ = ExceptionClass::as_r_class;
    let _ = ExceptionClass::raise;
    let _ = ExceptionClass::new_str;
    let _ = <RClass as Module>::define_class::<&core::ffi::CStr>;
    let _ = <RClass as Module>::define_module::<&core::ffi::CStr>;
    let _ = <RClass as Module>::define_error::<&core::ffi::CStr>;
    let _ = <RClass as Module>::class_get::<&core::ffi::CStr>;
    let _ = <RClass as Module>::module_get::<&core::ffi::CStr>;
    let _ = <RClass as Module>::define_method::<&core::ffi::CStr>;
    let _ = <RClass as Module>::define_private_method::<&core::ffi::CStr>;
    let _ = <RClass as Module>::define_module_function::<&core::ffi::CStr>;
    let _ = <RClass as Module>::define_const::<&core::ffi::CStr>;
    let _ = <RClass as Module>::name;
    let _ = <RModule as Module>::define_class::<&core::ffi::CStr>;
    let _ = <RModule as Module>::define_error::<&core::ffi::CStr>;
    let _ = <RModule as Module>::define_method::<&core::ffi::CStr>;
    let _ = <RModule as Module>::define_private_method::<&core::ffi::CStr>;
    let _ = <RModule as Module>::define_module_function::<&core::ffi::CStr>;
    let _ = <RModule as Module>::define_const::<&core::ffi::CStr>;
    let _ = <RClass as Object>::define_singleton_method::<&core::ffi::CStr>;
    let _ = <RModule as Object>::define_singleton_method::<&core::ffi::CStr>;
    let _ = <ExceptionClass as Module>::define_method::<&core::ffi::CStr>;
    let _ = <ExceptionClass as Object>::define_singleton_method::<&core::ffi::CStr>;
    let _ = <RObject as Object>::undef_singleton_method::<&core::ffi::CStr>;
    let _ = Error::message;
    let _ = Error::backtrace;
    let _ = Error::argnum;
    let _ = Error::is_kind_of::<RClass>;
    let _ = MethodDef::new;
    struct _SurfaceGem;
    impl Gem for _SurfaceGem {
        fn init(_mrb: &Mrb) -> Result<(), Error> {
            Ok(())
        }
    }
    let _ = Mrb::init_gem::<_SurfaceGem>;
    let _ = <_SurfaceGem as Gem>::init;
    fn _surface_typed(_mrb: &Mrb, _self: Value, _a: i32) -> i32 {
        0
    }
    fn _surface_any(_mrb: &Mrb, _self: Value) -> Value {
        Value::zeroed()
    }
    let _ = beni::method!(_surface_typed, 1);
    let _ = beni::method!(_surface_any, -1);
    let _ = RArray::from_value_unchecked;
    let _ = RArray::push;
    let _ = RArray::entry;
    let _ = RArray::entries;
    let _ = RArray::to_vec::<i64>;
    let _ = RArray::to_array::<i64, 1>;
    let _ = RArray::len;
    let _ = RArray::is_empty;
    let _ = RString::from_value_unchecked;
    let _ = RString::cat;
    let _ = RString::cat_str;
    let _ = RString::as_bytes;
    let _ = RString::to_char;
    let _ = RString::to_string;
    let _ = RString::len;
    let _ = RString::is_empty;
    let _ = Proc::from_value_unchecked;
    let _ = Proc::call;
    let _ = Proc::dump;
    let _ = RHash::from_value_unchecked;
    let _ = RHash::set;
    let _ = RHash::get;
    let _ = RHash::keys;
    let _ = RHash::to_hash_map::<i64, i64>;
    let _ = RHash::to_btree_map::<i64, i64>;
    let _ = Range::from_value_unchecked;
    let _ = Range::begin;
    let _ = Range::end;
    let _ = Range::is_exclusive;
    let _ = Range::beg_len;
    let _ = Symbol::from_value_unchecked;
    let _ = Symbol::new;
    let _ = <Value as ReprValue>::as_value;
    let _ = <Value as sys::AsRawValue>::as_raw;
    let _ = <RArray as ReprValue>::as_value;
    let _ = <RArray as sys::AsRawValue>::as_raw;
    let _ = <RHash as ReprValue>::as_value;
    let _ = <RHash as sys::AsRawValue>::as_raw;
    let _ = <Proc as ReprValue>::as_value;
    let _ = <Proc as sys::AsRawValue>::as_raw;
    let _ = <Range as ReprValue>::as_value;
    let _ = <Range as sys::AsRawValue>::as_raw;
    let _ = <RString as ReprValue>::as_value;
    let _ = <RString as sys::AsRawValue>::as_raw;
    let _ = <Symbol as ReprValue>::as_value;
    let _ = <Symbol as sys::AsRawValue>::as_raw;
    let _ = <RClass as ReprValue>::as_value;
    let _ = <RClass as sys::AsRawValue>::as_raw;
    let _ = <RModule as ReprValue>::as_value;
    let _ = <RModule as sys::AsRawValue>::as_raw;
    let _ = <ExceptionClass as ReprValue>::as_value;
    let _ = <ExceptionClass as sys::AsRawValue>::as_raw;
    let _ = <Id as sys::FromRawId>::from_raw;
    let _ = <Id as sys::AsRawId>::as_raw;
    let _ = <Symbol as From<Id>>::from;
    let _ = <Id as From<Symbol>>::from;
    let _ = Symbol::name;
    let _ = <&core::ffi::CStr as IntoId>::into_id;
    let _ = <&str as IntoId>::into_id;
    let _ = <String as IntoId>::into_id;
    let _ = <Id as IntoId>::into_id;
    let _ = <Symbol as IntoId>::into_id;
    let _ = ParseMessage::line;
    let _ = ParseMessage::column;
    let _ = ParseMessage::message;
    let _ = <i32 as IntoValue>::into_value;
    let _ = <f32 as IntoValue>::into_value;
    let _ = <bool as IntoValue>::into_value;
    let _ = <i32 as FromValue>::from_value;
    let _ = <f64 as FromValue>::from_value;
    let _ = <RArray as FromValue>::from_value;
    let _ = <RHash as FromValue>::from_value;
    let _ = <Range as FromValue>::from_value;
    let _ = <RClass as FromValue>::from_value;
    let _ = <RModule as FromValue>::from_value;
    let _ = <ExceptionClass as FromValue>::from_value;
    let _ = <Proc as FromValue>::from_value;
    let _ = <Symbol as FromValue>::from_value;
    let _ = <Symbol as IntoValue>::into_value;
    let _ = <RString as IntoValue>::into_value;
    let _ = <RArray as IntoValue>::into_value;
    let _ = <RHash as IntoValue>::into_value;
    let _ = <Proc as IntoValue>::into_value;
    let _ = <Range as IntoValue>::into_value;
    let _ = <RClass as IntoValue>::into_value;
    let _ = <RModule as IntoValue>::into_value;
    let _ = <ExceptionClass as IntoValue>::into_value;
    let _ = RArray::clear;
    let _ = RArray::concat;
    let _ = RArray::dup;
    let _ = RArray::join;
    let _ = RArray::pop;
    let _ = RArray::replace;
    let _ = RArray::resize;
    let _ = RArray::shift;
    let _ = RArray::splice;
    let _ = RArray::store;
    let _ = RArray::unshift;
    let _ = DataType::<u8>::new;
    let _ = Error::new;
    let _ = GcRoot::value;
    let _ = RHash::clear;
    let _ = RHash::contains_key;
    let _ = RHash::delete;
    let _ = RHash::dup;
    let _ = RHash::each::<fn(Value, Value) -> ForEach>;
    let _ = RHash::fetch;
    let _ = RHash::is_empty;
    let _ = RHash::len;
    let _ = RHash::update;
    let _ = RHash::values;
    let _ = MethodDef::new_with_block;
    let _ = MethodDef::new_with_opt;
    let _ = Mrb::arg1;
    let _ = Mrb::argc;
    let _ = Mrb::argv;
    let _ = Mrb::ary_new_capa;
    let _ = Mrb::ary_new_from_values;
    let _ = Mrb::assoc_new;
    let _ = Mrb::block_given;
    let _ = Mrb::carrier;
    let _ = Mrb::class_defined::<&core::ffi::CStr>;
    let _ = Mrb::class_new;
    let _ = Mrb::exc_get::<&core::ffi::CStr>;
    let _ = Mrb::full_gc;
    let _ = Mrb::gc_add_region;
    let _ = Mrb::gc_register_forever;
    let _ = Mrb::gc_root;
    let _ = Mrb::gv_remove::<&core::ffi::CStr>;
    let _ = Mrb::hash_new_capa;
    let _ = Mrb::incremental_gc;
    let _ = Mrb::intern;
    let _ = Mrb::intern_check;
    let _ = Mrb::intern_static;
    let _ = Mrb::mark_carrier;
    let _ = Mrb::module_new;
    let _ = Mrb::str_new_static;
    let _ = Mrb::sym_dump;
    let _ = Mrb::sym_name_len;
    let _ = Mrb::set_user_data::<u8>;
    let _ = Mrb::user_data::<u8>;
    let _ = Mrb::take_user_data::<u8>;
    let _ = RString::cat_cstr;
    let _ = RString::cmp;
    let _ = RString::concat;
    let _ = RString::dup;
    let _ = RString::eq;
    let _ = RString::index;
    let _ = RString::intern;
    let _ = RString::plus;
    let _ = RString::resize;
    let _ = RString::substr;
    let _ = RString::to_cstr;
    let _ = RString::to_f;
    let _ = RString::to_i;
    let _ = RString::to_inum;
    let _ = Symbol::dump;
    let _ = Symbol::name_bytes;
    let _ = Symbol::to_str;
    let _ = <Value as ReprValue>::add;
    let _ = Float::to_f64;
    let _ = Integer::to_i64;
    let _ = <Value as ReprValue>::check_frozen;
    let _ = <Value as ReprValue>::class;
    let _ = <Value as ReprValue>::cmp;
    let _ = <RClass as Module>::const_defined_at::<&core::ffi::CStr>;
    let _ = <RClass as Module>::const_remove::<&core::ffi::CStr>;
    let _ = <RClass as Module>::const_set::<&core::ffi::CStr, Value>;
    let _ = <RClass as Module>::cvar_defined::<&core::ffi::CStr>;
    let _ = <RClass as Module>::cvar_set::<&core::ffi::CStr, Value>;
    let _ = <RObject as Object>::ivar_foreach::<fn(Symbol, Value) -> ForEach>;
    let _ = <Value as ReprValue>::eql;
    let _ = <Value as ReprValue>::equal;
    let _ = Float::to_integer;
    let _ = <Value as ReprValue>::freeze;
    let _ = <Value as sys::FromRawValue>::from_raw;
    let _ = <Value as ReprValue>::funcall_with_block::<&core::ffi::CStr>;
    let _ = <Value as ReprValue>::inspect;
    let _ = Integer::to_r_string_radix;
    let _ = <Value as ReprValue>::is_instance_of::<RClass>;
    let _ = <Value as ReprValue>::is_kind_of::<RClass>;
    let _ = <RObject as Object>::ivar_remove::<&core::ffi::CStr>;
    let _ = <Value as ReprValue>::mul;
    let _ = <Value as ReprValue>::is_equal;
    let _ = <Value as ReprValue>::object_id;
    let _ = <RObject as Object>::singleton_class;
    let _ = <Value as ReprValue>::sub;
    let _ = RArray::to_ary;
    let _ = <Value as ReprValue>::to_bool;
    let _ = <Value as ReprValue>::to_sym;
}

#[test]
fn typed_mrb_func_t_coerces_from_value_bridge() {
    // Companion to beni-sys's `mrb_func_t_is_a_valid_extern_c_fn_
    // pointer`: a function declared with the typed `Value`-based
    // signature must coerce to `beni::mrb_func_t` without a cast.
    // Should `Value`'s `#[repr(transparent)]` over `mrb_value` ever
    // drift, the `transmute` inside `Module::define_method` becomes
    // UB — this and `value_shares_abi_with_mrb_value` are the guard
    // rail.
    unsafe extern "C" fn _stub(_mrb: *mut beni::sys::mrb_state, _self_: Value) -> Value {
        Value::zeroed()
    }
    let _f: beni::mrb_func_t = _stub;
}

/// The helpers `beni::sys` carries beside the raw bindings are free
/// functions, which `rake api:surface` does not list, so each is named
/// here by hand.
#[test]
fn raw_layer_helpers_are_reachable_from_outside() {
    let _ = beni::sys::protect::<fn(&Mrb) -> Value>;
    let _ = beni::sys::catch_unwind::<fn() -> u8, u8>;
}

/// The drift net for the `compiler` capability feature. An item the
/// feature carries is named here rather than above, so the ungated net
/// stays whole for a consumer who never enables it and `rake
/// api:surface` can tell the two apart.
#[cfg(feature = "compiler")]
#[test]
fn compiler_surface_is_reachable_from_outside() {
    let _ = Ccontext::new;
    let _ = Ccontext::load_nstring;
    let _ = Ccontext::compile;
    let _ = Ccontext::warnings;
    let _ = Mrb::load_string;
}

/// The `bytes` dependency feature's inherent surface, gated as the
/// feature gates it.
#[cfg(feature = "bytes")]
#[test]
fn bytes_surface_is_reachable_from_outside() {
    let _ = RString::to_bytes;
}
