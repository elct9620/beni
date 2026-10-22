//! Debug hooks — mirrors `magnus::debug`.
//!
//! An archive built with `MRB_USE_DEBUG_HOOK` calls a code fetch hook
//! before each instruction it runs; mruby's own debugger stops at
//! breakpoints from it. The owner installs a closure as the hook
//! through `Mrb::set_code_fetch_hook`, and each call hands it a
//! `TraceArg` reading the instruction about to run and, through a
//! `DebugInspector`, the frames of the fiber running it.
//!
//! Neither view offers an interpreter handle, so a hook runs no Ruby.
//! The VM calls the hook holding a pointer into its frame stack, and a
//! Ruby call deep enough to grow that stack moves it, leaving the VM to
//! run on from freed memory once the hook returns (`vendor/mruby/src/vm.c`,
//! `CALL_CODE_HOOKS` beside `cipush`).

use crate::method::{BlockReturn, InitReturn};
use crate::sys::AsRawValue;
use crate::value::ReprValue;
use crate::{sys, ClassOrModule, Error, FromValue, Id, Mrb, Value};
use core::marker::PhantomData;
use core::num::NonZero;
use std::borrow::Cow;
use std::ffi::CStr;

/// The closure a code fetch hook runs, with its return projected.
pub(crate) type Hook = Box<dyn FnMut(TraceArg<'_>) -> Result<(), Error> + Send>;

impl Mrb {
    /// Install `hook` as this interpreter's code fetch hook, dropping
    /// any hook installed before it. The interpreter calls it before
    /// each instruction it runs, in whichever fiber runs it.
    ///
    /// An `Err` from the hook, or a panic in it, is raised in place of
    /// the instruction about to run, where the running code may rescue
    /// it.
    ///
    /// Only the owner installs — a registered method holds a borrow:
    ///
    /// ```compile_fail
    /// fn install_through_a_borrow(mrb: &beni::Mrb) {
    ///     mrb.set_code_fetch_hook(|_| ());
    /// }
    /// ```
    ///
    /// The hook outlives any borrow, so it cannot carry a handle to call
    /// Ruby through:
    ///
    /// ```compile_fail
    /// fn capture_a_handle(mrb: &mut beni::Mrb, other: &'static beni::Mrb) {
    ///     mrb.set_code_fetch_hook(move |_| other.full_gc());
    /// }
    /// ```
    pub fn set_code_fetch_hook<F, R>(&mut self, mut hook: F)
    where
        F: FnMut(TraceArg<'_>) -> R + Send + 'static,
        R: InitReturn,
    {
        let hook: Hook = Box::new(move |arg| hook(arg).into_init_return());
        *self.slot_mut().hook.get_mut() = Some(hook);
        // SAFETY: `self` is live, and the slot now holds the closure the
        // trampoline runs.
        unsafe { (*self.as_ptr()).code_fetch_hook = Some(code_fetch_hook) };
    }

    /// Drop the installed code fetch hook. Nothing changes when none is
    /// installed.
    pub fn remove_code_fetch_hook(&mut self) {
        // SAFETY: `self` is live; clearing the field first means the
        // interpreter calls no hook once the closure is dropped.
        unsafe { (*self.as_ptr()).code_fetch_hook = None };
        *self.slot_mut().hook.get_mut() = None;
    }

    /// Run `func` with a `DebugInspector` over the running fiber's frames,
    /// index `0` being the frame running the Rust code that calls this,
    /// and answer what it returns. Mirrors magnus's
    /// `Ruby::debug_inspector_open`.
    pub fn debug_inspector_open<F, R>(&self, func: F) -> Result<Value, Error>
    where
        F: FnOnce(DebugInspector<'_>) -> R,
        R: BlockReturn,
    {
        let inspector = DebugInspector {
            mrb: self.as_ptr(),
            holds: true,
            _call: PhantomData,
        };
        crate::method::private::BlockReturn::into_block_return(func(inspector), self)
    }

    /// How many frames the running fiber holds, its outermost included.
    /// Mirrors magnus's `Ruby::debug_inspector_current_depth`.
    pub fn debug_inspector_current_depth(&self) -> usize {
        let inspector = DebugInspector {
            mrb: self.as_ptr(),
            holds: false,
            _call: PhantomData,
        };
        inspector.running_depth() + 1
    }
}

/// What mruby calls before each instruction: runs the installed closure
/// under the panic boundary, raising what it answers in place of the
/// instruction.
unsafe extern "C" fn code_fetch_hook(
    mrb: *mut sys::mrb_state,
    irep: *const sys::mrb_irep,
    pc: *const sys::mrb_code,
    _regs: *mut sys::mrb_value,
) {
    // SAFETY: mruby calls the hook with a live state pointer that
    // outlives the call, and only an owner installs the hook, so `ud` is
    // that owner's slot.
    let mrb = unsafe { Mrb::borrow_raw(&mrb) };
    let Some(slot) = mrb.slot() else { return };
    // SAFETY: only this trampoline reaches the hook through a borrow, and
    // the hook runs no Ruby, so no other call of it is live. The owner
    // replaces it only through `&mut Mrb`, which no running VM leaves.
    let Some(hook) = (unsafe { &mut *slot.hook.get() }) else {
        return;
    };
    let arg = TraceArg {
        mrb: mrb.as_ptr(),
        irep,
        pc,
        _call: PhantomData,
    };
    // mruby finds the rescue covering a raise, and the line its backtrace
    // names, from the frame's pc taken as already past the instruction
    // that raised (`catch_handler_find` in `vendor/mruby/src/vm.c`, and
    // the `pc[-1]` of `src/backtrace.c`). The hook runs before the
    // instruction, so its pc stands one byte into it while the hook
    // runs, and a raise lands on this instruction rather than the one
    // before; a hook that returns puts it back for the VM to decode.
    // SAFETY: the running frame is the one `pc` stands in, an
    // instruction is at least one byte long, and the hook runs no Ruby,
    // so the frame stays where it is until the pc is put back.
    let frame = unsafe { (*(*mrb.as_ptr()).c).ci };
    unsafe { (*frame).pc = pc.add(1) };
    // SAFETY: this is a bridge frame mruby may unwind out of, holding
    // nothing that needs `Drop`.
    unsafe {
        crate::method::handle_error(mrb, || {
            hook(arg)?;
            Ok(crate::value::qnil().as_value())
        })
    };
    // SAFETY: as above.
    unsafe { (*frame).pc = pc };
}

/// The instruction a code fetch hook was called before — magnus's
/// `TraceArg`. Valid only for that call.
#[derive(Clone, Copy)]
pub struct TraceArg<'a> {
    mrb: *mut sys::mrb_state,
    irep: *const sys::mrb_irep,
    pc: *const sys::mrb_code,
    _call: PhantomData<&'a ()>,
}

impl<'a> TraceArg<'a> {
    /// The line of the instruction, or nothing for code that carries no
    /// line numbers.
    pub fn lineno(self) -> Option<NonZero<usize>> {
        // SAFETY: the hook call keeps the state, the irep, and the pc
        // live.
        unsafe { line_at(self.mrb, self.irep, self.pc) }
    }

    /// The file name of the instruction's code, or nothing for code that
    /// carries no line numbers. A string slice where magnus answers an
    /// `RString`: mruby's debug info keeps the name as a C string
    /// (`mrb_debug_get_filename`), and the hook holds no interpreter to
    /// make a String with.
    pub fn path(self) -> Option<Cow<'a, str>> {
        // SAFETY: as `lineno`.
        unsafe { path_at(self.mrb, self.irep, self.pc) }
    }

    /// The name of the method running the instruction, or nothing at the
    /// top level. An `Id` where magnus answers a `StaticSymbol`: mruby
    /// keeps every symbol until the interpreter closes (`mrb_free_symtbl`
    /// in `vendor/mruby/src/state.c`), so no symbol is dynamic.
    pub fn method_id(self) -> Option<Id> {
        self.debug_inspector().frame_method_id(0)
    }

    /// The `self` of the code running the instruction.
    pub fn tracearg_self(self) -> Value {
        // SAFETY: the hook call keeps the running frame and its registers
        // live, and register `0` of every frame is its `self`.
        let frame = unsafe { &*(*(*self.mrb).c).ci };
        Value::from_raw_unchecked(unsafe { *frame.stack })
    }

    /// The class or module defining the method running the instruction,
    /// or nothing at the top level. A `ClassOrModule` where magnus answers
    /// an `RClass`: the method may be a module's, and an `RClass` holds a
    /// class alone.
    pub fn defined_class(self) -> Option<ClassOrModule> {
        self.debug_inspector().frame_class_get(0)
    }

    /// The frames of the fiber running the instruction, valid for the
    /// same call. magnus's hook reaches them through `Ruby`, which this
    /// hook does not hold.
    pub fn debug_inspector(self) -> DebugInspector<'a> {
        DebugInspector {
            mrb: self.mrb,
            holds: false,
            _call: PhantomData,
        }
    }
}

/// The frames of the fiber a code fetch hook was called in — magnus's
/// `DebugInspector`. Index `0` is the frame running the instruction and
/// each next index the frame that called it; an index past the fiber's
/// outermost frame answers nothing. Reads only.
///
/// magnus answers each read as a `Result`, for CRuby's inspector raises
/// on an index it has no frame for. mruby has no inspector API; the reads
/// walk the fiber's frame stack (`mrb_context` in `include/mruby.h`) and
/// raise nothing, so an index past it answers `None`.
#[derive(Clone, Copy)]
pub struct DebugInspector<'a> {
    mrb: *mut sys::mrb_state,
    /// Whether a read holds what it answers in the arena: opened from
    /// Rust code, the closure may call Ruby before it uses a value, and
    /// a hook runs none.
    holds: bool,
    _call: PhantomData<&'a ()>,
}

impl<'a> DebugInspector<'a> {
    /// How many frames lie beneath the indexed one, `0` for the
    /// outermost.
    pub fn frame_depth(self, index: usize) -> Option<usize> {
        self.running_depth().checked_sub(index)
    }

    /// The file name of the indexed frame's code, or nothing for code
    /// that carries no line numbers or a frame running Rust or C code.
    pub fn frame_path(self, index: usize) -> Option<Cow<'a, str>> {
        let (irep, pc) = self.position(index)?;
        // SAFETY: a read runs no Ruby, so the frame's code stays live.
        unsafe { path_at(self.mrb, irep, pc) }
    }

    /// The line of the indexed frame's current instruction or pending
    /// call, or nothing for code that carries no line numbers or a frame
    /// running Rust or C code.
    pub fn frame_lineno(self, index: usize) -> Option<NonZero<usize>> {
        let (irep, pc) = self.position(index)?;
        // SAFETY: as `frame_path`.
        unsafe { line_at(self.mrb, irep, pc) }
    }

    /// The name of the indexed frame's method, or nothing at the top
    /// level.
    pub fn frame_method_id(self, index: usize) -> Option<Id> {
        let mid = self.frame(index)?.mid;
        (mid != 0).then(|| Id::from_raw_unchecked(mid))
    }

    /// The `self` of the indexed frame.
    pub fn frame_self_get(self, index: usize) -> Option<Value> {
        let frame = self.frame(index)?;
        // SAFETY: register `0` of every frame is its `self`, and the frame
        // lies in the stack the call keeps in place.
        Some(self.handed(Value::from_raw_unchecked(unsafe { *frame.stack })))
    }

    /// The class or module defining the indexed frame's method, or nothing
    /// at the top level. A `ClassOrModule` where magnus answers an
    /// `RClass`, as `TraceArg::defined_class` does.
    pub fn frame_class_get(self, index: usize) -> Option<ClassOrModule> {
        let frame = self.frame(index)?;
        if frame.mid == 0 {
            return None;
        }
        // SAFETY: `frame` lies in the running fiber's frame stack, which
        // the call keeps in place.
        let class = defining(unsafe { sys::mrb_vm_ci_target_class(frame) })?;
        self.handed(class.as_value());
        Some(class)
    }

    /// Each local variable the indexed frame's code can name, with its
    /// value: its own scope's, then each enclosing scope's out to the
    /// nearest scope start, an inner variable hiding an outer one of the
    /// same name — the set mruby's `local_variables` lists. A frame
    /// running Rust or C code, or code compiled without its local
    /// variable names, declares none.
    pub fn frame_local_variables(self, index: usize) -> Option<Vec<(Id, Value)>> {
        let frame = self.frame(index)?;
        let mut locals: Vec<(Id, Value)> = Vec::new();
        // SAFETY: `frame` lies in the running fiber's frame stack, which
        // stays in place while a read runs no Ruby.
        let mut irep = unsafe { sys::mrb_ci_irep_func(frame) };
        let mut proc_ = frame.proc_;
        // The frame's registers hold `self` then each of its own locals.
        let mut values = frame.stack.cast_const();
        let mut held = usize::MAX;
        // SAFETY: a non-NULL irep is the live code of the scope `proc_`
        // runs, kept by the frame's proc or the procs it encloses.
        while let Some(code) = unsafe { irep.as_ref() } {
            if !code.lv.is_null() {
                let count = usize::from(code.nlocals).saturating_sub(1);
                // SAFETY: mruby sizes `lv` to one name per local,
                // `nlocals - 1` of them.
                let names = unsafe { core::slice::from_raw_parts(code.lv, count) };
                for (slot, &name) in names.iter().enumerate().take(held) {
                    if name == 0
                        || self.is_anonymous(name)
                        || locals.iter().any(|(seen, _)| seen.to_raw() == name)
                    {
                        continue;
                    }
                    // SAFETY: `values` holds `self` then `held` locals, in
                    // the order `lv` names them, and `slot` is below `held`.
                    let raw = unsafe { *values.add(slot + 1) };
                    locals.push((
                        Id::from_raw_unchecked(name),
                        self.handed(Value::from_raw_unchecked(raw)),
                    ));
                }
            }
            let mut env = core::ptr::null_mut();
            // SAFETY: `proc_` is the live proc running `code`.
            let upper = unsafe { sys::mrb_proc_enclosing_func(proc_, &mut env) };
            if upper.is_null() {
                break;
            }
            // SAFETY: a non-NULL environment is the live one `proc_`
            // captured, its stack holding `self` then the enclosing
            // scope's locals, as many values as its length counts.
            (values, held) = match unsafe { env.as_ref() } {
                Some(env) => (env.stack.cast_const(), unsafe {
                    (sys::mrb_env_len_func(env) as usize).saturating_sub(1)
                }),
                None => (core::ptr::null(), 0),
            };
            proc_ = upper;
            // SAFETY: `upper` is the live proc of the enclosing scope.
            irep = unsafe { sys::mrb_proc_irep_func(sys::mrb_obj_value(upper.cast_mut().cast())) };
        }
        Some(locals)
    }

    /// Whether `name` is an anonymous parameter's marker (`*`, `**`, `&`),
    /// which mruby's `local_variables` names no variable
    /// (`mrb_proc_local_variables` in `vendor/mruby/src/proc.c`).
    fn is_anonymous(self, name: sys::mrb_sym) -> bool {
        let mut len = 0;
        // SAFETY: the call keeps the state live, and `name` was interned
        // against it.
        let ptr = unsafe { sys::mrb_sym_name_len(self.mrb, name, &mut len) };
        // SAFETY: a non-NULL name spans `len` bytes, read here before any
        // other name read can reuse the buffer a short name unpacks into.
        !ptr.is_null() && len > 0 && matches!(unsafe { *ptr.cast::<u8>() }, b'*' | b'&')
    }

    /// `value`, held in the arena when this inspector holds its reads.
    fn handed(self, value: Value) -> Value {
        if self.holds {
            // SAFETY: the call keeps the state live, and protecting an
            // immediate is a no-op.
            unsafe { sys::mrb_gc_protect(self.mrb, value.as_raw()) };
        }
        value
    }

    /// The running fiber's frame stack: its base and its running frame.
    fn frames(self) -> (*const sys::mrb_callinfo, *const sys::mrb_callinfo) {
        // SAFETY: a read runs no Ruby, so the state and its running
        // context stay live.
        let context = unsafe { &*(*self.mrb).c };
        (context.cibase, context.ci)
    }

    /// How many frames lie beneath the running one.
    fn running_depth(self) -> usize {
        let (base, running) = self.frames();
        // SAFETY: both point into the same frame stack, the running
        // frame at or above its base.
        unsafe { running.offset_from(base) as usize }
    }

    fn frame(self, index: usize) -> Option<&'a sys::mrb_callinfo> {
        let depth = self.frame_depth(index)?;
        let (base, _) = self.frames();
        // SAFETY: `depth` is at most the running frame's, so the frame
        // lies within the stack, which stays in place while a read runs
        // no Ruby.
        Some(unsafe { &*base.add(depth) })
    }

    /// The indexed frame's code and the instruction it stands at: the
    /// one about to run for the running frame, the call it waits on for
    /// any other.
    fn position(self, index: usize) -> Option<(*const sys::mrb_irep, *const sys::mrb_code)> {
        let frame = self.frame(index)?;
        // SAFETY: as `frame_local_variables`.
        let irep = unsafe { sys::mrb_ci_irep_func(frame) };
        // SAFETY: a non-NULL irep is the frame's live code.
        if irep.is_null() || frame.pc.is_null() || frame.pc <= unsafe { (*irep).iseq } {
            return None;
        }
        // SAFETY: every frame a read reaches stands past the start of its
        // instruction — a caller has advanced past the call it waits on,
        // and the code fetch hook steps the running frame one byte into
        // the instruction about to run — so one byte back lies within it,
        // and within the code.
        Some((irep, unsafe { frame.pc.sub(1) }))
    }
}

/// The class or module `class` stands for. A method's frame holds the
/// class its method was found in, which for a module's method or a class
/// with prepended modules is an include class; an include class points at
/// the class or module it was made from (`include_class_new` and
/// `prepend_module` in `vendor/mruby/src/class.c`).
fn defining(class: *mut sys::RClass) -> Option<ClassOrModule> {
    if class.is_null() {
        return None;
    }
    // SAFETY: a non-NULL target class is a live class-family object, and
    // `mrb_obj_value` only boxes the pointer.
    let mut value = Value::from_raw_unchecked(unsafe { sys::mrb_obj_value(class.cast()) });
    if value.tag() == sys::MRB_TT_ICLASS {
        // SAFETY: an include class's `c` is the live class or module it
        // was made from.
        let real = unsafe { (*class.cast::<sys::RBasic>()).c };
        value = Value::from_raw_unchecked(unsafe { sys::mrb_obj_value(real.cast()) });
    }
    ClassOrModule::from_value(value)
}

/// The line of `pc` in `irep`, or nothing when it carries no lines.
///
/// # Safety
///
/// `mrb`, `irep`, and `pc` are live, `pc` within `irep`'s code.
unsafe fn line_at(
    mrb: *mut sys::mrb_state,
    irep: *const sys::mrb_irep,
    pc: *const sys::mrb_code,
) -> Option<NonZero<usize>> {
    // SAFETY: forwarded from the caller.
    let line = unsafe { sys::mrb_debug_get_line(mrb, irep, offset(irep, pc)) };
    NonZero::new(usize::try_from(line).ok()?)
}

/// The file name of `pc` in `irep`, or nothing when it carries none.
///
/// # Safety
///
/// As `line_at`; the name lives as long as `irep`.
unsafe fn path_at<'a>(
    mrb: *mut sys::mrb_state,
    irep: *const sys::mrb_irep,
    pc: *const sys::mrb_code,
) -> Option<Cow<'a, str>> {
    // SAFETY: forwarded from the caller.
    let name = unsafe { sys::mrb_debug_get_filename(mrb, irep, offset(irep, pc)) };
    // SAFETY: a non-NULL name is a NUL-terminated string the irep's
    // debug info owns.
    (!name.is_null()).then(|| unsafe { CStr::from_ptr(name) }.to_string_lossy())
}

/// The index of `pc` in `irep`'s instructions, the form mruby's debug
/// reads take.
///
/// # Safety
///
/// `pc` lies within `irep`'s code.
unsafe fn offset(irep: *const sys::mrb_irep, pc: *const sys::mrb_code) -> u32 {
    // SAFETY: forwarded from the caller.
    unsafe { pc.offset_from((*irep).iseq) as u32 }
}
