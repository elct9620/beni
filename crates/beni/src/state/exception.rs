//! Accessors for the exception classes mruby's core defines, named as
//! magnus's `Ruby::exception_*`. `Exception` and `StandardError` are
//! fields of the interpreter (`E_EXCEPTION` / `E_STANDARD_ERROR`), read
//! without failing. mruby resolves every other one by its constant on
//! each use (`E_ARGUMENT_ERROR` and its kin are `mrb_exc_get_id`), so
//! their accessors are `Mrb::exc_get` under the class's name.

use crate::{Error, ExceptionClass, Mrb};

impl Mrb {
    /// `Exception`, which the interpreter holds itself. Mirrors magnus's
    /// `Ruby::exception_exception`.
    #[inline]
    pub fn exception_exception(&self) -> ExceptionClass {
        // SAFETY: `self` is alive by the `&self` borrow; the field is set
        // when the interpreter opens and names `Exception` from then on.
        ExceptionClass::from_raw_unchecked(unsafe { (*self.as_ptr()).eException_class })
    }

    /// `StandardError`, which the interpreter holds itself. Mirrors
    /// magnus's `Ruby::exception_standard_error`.
    #[inline]
    pub fn exception_standard_error(&self) -> ExceptionClass {
        // SAFETY: as `exception_exception`; the field names
        // `StandardError`, a class descending from `Exception`.
        ExceptionClass::from_raw_unchecked(unsafe { (*self.as_ptr()).eStandardError_class })
    }

    /// `ArgumentError`, as `exc_get` finds it. Mirrors magnus's
    /// `Ruby::exception_arg_error`.
    #[inline]
    pub fn exception_arg_error(&self) -> Result<ExceptionClass, Error> {
        self.exc_get(c"ArgumentError")
    }

    /// `FloatDomainError`, as `exc_get` finds it. Mirrors magnus's
    /// `Ruby::exception_float_domain_error`.
    #[inline]
    pub fn exception_float_domain_error(&self) -> Result<ExceptionClass, Error> {
        self.exc_get(c"FloatDomainError")
    }

    /// `FrozenError`, as `exc_get` finds it. Mirrors magnus's
    /// `Ruby::exception_frozen_error`.
    #[inline]
    pub fn exception_frozen_error(&self) -> Result<ExceptionClass, Error> {
        self.exc_get(c"FrozenError")
    }

    /// `IndexError`, as `exc_get` finds it. Mirrors magnus's
    /// `Ruby::exception_index_error`.
    #[inline]
    pub fn exception_index_error(&self) -> Result<ExceptionClass, Error> {
        self.exc_get(c"IndexError")
    }

    /// `KeyError`, as `exc_get` finds it. Mirrors magnus's
    /// `Ruby::exception_key_error`.
    #[inline]
    pub fn exception_key_error(&self) -> Result<ExceptionClass, Error> {
        self.exc_get(c"KeyError")
    }

    /// `LocalJumpError`, as `exc_get` finds it. Mirrors magnus's
    /// `Ruby::exception_local_jump_error`.
    #[inline]
    pub fn exception_local_jump_error(&self) -> Result<ExceptionClass, Error> {
        self.exc_get(c"LocalJumpError")
    }

    /// `NameError`, as `exc_get` finds it. Mirrors magnus's
    /// `Ruby::exception_name_error`.
    #[inline]
    pub fn exception_name_error(&self) -> Result<ExceptionClass, Error> {
        self.exc_get(c"NameError")
    }

    /// `NoMatchingPatternError`, as `exc_get` finds it. Mirrors magnus's
    /// `Ruby::exception_no_matching_pattern_error`.
    #[inline]
    pub fn exception_no_matching_pattern_error(&self) -> Result<ExceptionClass, Error> {
        self.exc_get(c"NoMatchingPatternError")
    }

    /// `NoMemoryError`, as `exc_get` finds it. Mirrors magnus's
    /// `Ruby::exception_no_mem_error`.
    #[inline]
    pub fn exception_no_mem_error(&self) -> Result<ExceptionClass, Error> {
        self.exc_get(c"NoMemoryError")
    }

    /// `NoMethodError`, as `exc_get` finds it. Mirrors magnus's
    /// `Ruby::exception_no_method_error`.
    #[inline]
    pub fn exception_no_method_error(&self) -> Result<ExceptionClass, Error> {
        self.exc_get(c"NoMethodError")
    }

    /// `NotImplementedError`, as `exc_get` finds it. Mirrors magnus's
    /// `Ruby::exception_not_imp_error`.
    #[inline]
    pub fn exception_not_imp_error(&self) -> Result<ExceptionClass, Error> {
        self.exc_get(c"NotImplementedError")
    }

    /// `RangeError`, as `exc_get` finds it. Mirrors magnus's
    /// `Ruby::exception_range_error`.
    #[inline]
    pub fn exception_range_error(&self) -> Result<ExceptionClass, Error> {
        self.exc_get(c"RangeError")
    }

    /// `RegexpError`, as `exc_get` finds it. Mirrors magnus's
    /// `Ruby::exception_regexp_error`.
    #[inline]
    pub fn exception_regexp_error(&self) -> Result<ExceptionClass, Error> {
        self.exc_get(c"RegexpError")
    }

    /// `RuntimeError`, as `exc_get` finds it. Mirrors magnus's
    /// `Ruby::exception_runtime_error`.
    #[inline]
    pub fn exception_runtime_error(&self) -> Result<ExceptionClass, Error> {
        self.exc_get(c"RuntimeError")
    }

    /// `ScriptError`, as `exc_get` finds it. Mirrors magnus's
    /// `Ruby::exception_script_error`.
    #[inline]
    pub fn exception_script_error(&self) -> Result<ExceptionClass, Error> {
        self.exc_get(c"ScriptError")
    }

    /// `StopIteration`, as `exc_get` finds it. Mirrors magnus's
    /// `Ruby::exception_stop_iteration`.
    #[inline]
    pub fn exception_stop_iteration(&self) -> Result<ExceptionClass, Error> {
        self.exc_get(c"StopIteration")
    }

    /// `SyntaxError`, as `exc_get` finds it. Mirrors magnus's
    /// `Ruby::exception_syntax_error`.
    #[inline]
    pub fn exception_syntax_error(&self) -> Result<ExceptionClass, Error> {
        self.exc_get(c"SyntaxError")
    }

    /// `SystemStackError`, as `exc_get` finds it. Mirrors magnus's
    /// `Ruby::exception_sys_stack_error`.
    #[inline]
    pub fn exception_sys_stack_error(&self) -> Result<ExceptionClass, Error> {
        self.exc_get(c"SystemStackError")
    }

    /// `TypeError`, as `exc_get` finds it. Mirrors magnus's
    /// `Ruby::exception_type_error`.
    #[inline]
    pub fn exception_type_error(&self) -> Result<ExceptionClass, Error> {
        self.exc_get(c"TypeError")
    }

    /// `ZeroDivisionError`, as `exc_get` finds it. Mirrors magnus's
    /// `Ruby::exception_zero_div_error`.
    #[inline]
    pub fn exception_zero_div_error(&self) -> Result<ExceptionClass, Error> {
        self.exc_get(c"ZeroDivisionError")
    }
}
