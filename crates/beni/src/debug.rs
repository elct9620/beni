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

use crate::method::InitReturn;
use crate::{sys, Error, Id, Mrb, Value};
use core::marker::PhantomData;
use core::num::NonZero;
use std::borrow::Cow;
use std::ffi::CStr;
use std::panic::AssertUnwindSafe;

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
        if self.slot().is_some() {
            *self.slot_mut().hook.get_mut() = None;
        }
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
    // outlives the call.
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
    let Err(err) = crate::sys::catch_unwind(AssertUnwindSafe(|| hook(arg))).and_then(|res| res)
    else {
        return;
    };
    // mruby finds the rescue covering a raise, and the line its backtrace
    // names, from the frame's pc taken as already past the instruction
    // that raised (`catch_handler_find` in `vendor/mruby/src/vm.c`, and
    // the `pc[-1]` of `src/backtrace.c`). The hook runs before the
    // instruction, so its pc moves one byte into it first; the raise then
    // lands on this instruction rather than the one before.
    // SAFETY: the running frame is the one `pc` stands in, and an
    // instruction is at least one byte long.
    unsafe { (*(*(*mrb.as_ptr()).c).ci).pc = pc.add(1) };
    // SAFETY: this is a bridge frame mruby may unwind out of, holding
    // nothing that needs `Drop`.
    unsafe { crate::method::raise_error(mrb, err) }
}

/// The instruction a code fetch hook was called before — magnus's
/// `TraceArg`. Valid only for that call.
pub struct TraceArg<'a> {
    mrb: *mut sys::mrb_state,
    irep: *const sys::mrb_irep,
    pc: *const sys::mrb_code,
    _call: PhantomData<&'a ()>,
}

impl<'a> TraceArg<'a> {
    /// The line of the instruction, or nothing for code that carries no
    /// line numbers.
    pub fn lineno(&self) -> Option<NonZero<usize>> {
        // SAFETY: the hook call keeps the state, the irep, and the pc
        // live.
        unsafe { line_at(self.mrb, self.irep, self.pc) }
    }

    /// The file name of the instruction's code, or nothing for code
    /// loaded without one.
    pub fn path(&self) -> Option<Cow<'a, str>> {
        // SAFETY: as `lineno`.
        unsafe { path_at(self.mrb, self.irep, self.pc) }
    }

    /// The name of the method running the instruction, or nothing at the
    /// top level.
    pub fn method_id(&self) -> Option<Id> {
        self.debug_inspector().frame_method_id(0)
    }

    /// The frames of the fiber running the instruction, valid for the
    /// same call.
    pub fn debug_inspector(&self) -> DebugInspector<'a> {
        DebugInspector {
            mrb: self.mrb,
            _call: PhantomData,
        }
    }
}

/// The frames of the fiber a code fetch hook was called in — magnus's
/// `DebugInspector`. Index `0` is the frame running the instruction and
/// each next index the frame that called it; an index past the fiber's
/// outermost frame answers nothing. Reads only.
pub struct DebugInspector<'a> {
    mrb: *mut sys::mrb_state,
    _call: PhantomData<&'a ()>,
}

impl<'a> DebugInspector<'a> {
    /// How many frames lie beneath the indexed one, `0` for the
    /// outermost.
    pub fn frame_depth(&self, index: usize) -> Option<usize> {
        self.running_depth().checked_sub(index)
    }

    /// The file name of the indexed frame's code, or nothing for code
    /// loaded without one or a frame running Rust or C code.
    pub fn frame_path(&self, index: usize) -> Option<Cow<'a, str>> {
        let (irep, pc) = self.position(index)?;
        // SAFETY: the hook call keeps the frame's code live.
        unsafe { path_at(self.mrb, irep, pc) }
    }

    /// The line of the indexed frame's current instruction or pending
    /// call, or nothing for code that carries no line numbers or a frame
    /// running Rust or C code.
    pub fn frame_lineno(&self, index: usize) -> Option<NonZero<usize>> {
        let (irep, pc) = self.position(index)?;
        // SAFETY: as `frame_path`.
        unsafe { line_at(self.mrb, irep, pc) }
    }

    /// The name of the indexed frame's method, or nothing at the top
    /// level.
    pub fn frame_method_id(&self, index: usize) -> Option<Id> {
        let mid = self.frame(index)?.mid;
        (mid != 0).then(|| Id::from_raw_unchecked(mid))
    }

    /// Each local variable the indexed frame's code declares, in order,
    /// with its current value. A frame running Rust or C code, or code
    /// compiled without its local variable names, declares none.
    pub fn frame_local_variables(&self, index: usize) -> Option<Vec<(Id, Value)>> {
        let frame = self.frame(index)?;
        // SAFETY: `frame` lies in the running fiber's frame stack, which
        // the hook call keeps in place.
        let irep = unsafe { sys::mrb_ci_irep_func(frame) };
        // SAFETY: a non-NULL irep is the frame's live code.
        let Some(irep) = (unsafe { irep.as_ref() }) else {
            return Some(Vec::new());
        };
        if irep.lv.is_null() {
            return Some(Vec::new());
        }
        let count = usize::from(irep.nlocals).saturating_sub(1);
        // SAFETY: mruby sizes `lv` to one name per local, `nlocals - 1`
        // of them, and the frame's registers hold `self` then each local
        // in that order.
        let names = unsafe { core::slice::from_raw_parts(irep.lv, count) };
        let locals = names
            .iter()
            .enumerate()
            .filter(|(_, &name)| name != 0)
            .map(|(slot, &name)| {
                // SAFETY: register `slot + 1` is the local `name` names.
                let raw = unsafe { *frame.stack.add(slot + 1) };
                (Id::from_raw_unchecked(name), Value::from_raw_unchecked(raw))
            })
            .collect();
        Some(locals)
    }

    /// The running fiber's frame stack: its base and its running frame.
    fn frames(&self) -> (*const sys::mrb_callinfo, *const sys::mrb_callinfo) {
        // SAFETY: the hook call keeps the state and its running context
        // live.
        let context = unsafe { &*(*self.mrb).c };
        (context.cibase, context.ci)
    }

    /// How many frames lie beneath the running one.
    fn running_depth(&self) -> usize {
        let (base, running) = self.frames();
        // SAFETY: both point into the same frame stack, the running
        // frame at or above its base.
        unsafe { running.offset_from(base) as usize }
    }

    fn frame(&self, index: usize) -> Option<&'a sys::mrb_callinfo> {
        let depth = self.frame_depth(index)?;
        let (base, _) = self.frames();
        // SAFETY: `depth` is at most the running frame's, so the frame
        // lies within the stack the hook call keeps in place.
        Some(unsafe { &*base.add(depth) })
    }

    /// The indexed frame's code and the instruction it stands at: the
    /// one about to run for the running frame, the call it waits on for
    /// any other.
    fn position(&self, index: usize) -> Option<(*const sys::mrb_irep, *const sys::mrb_code)> {
        let frame = self.frame(index)?;
        // SAFETY: as `frame_local_variables`.
        let irep = unsafe { sys::mrb_ci_irep_func(frame) };
        if irep.is_null() || frame.pc.is_null() {
            return None;
        }
        // SAFETY: a frame that called another has advanced its pc past
        // that call, so one instruction back is the call itself.
        let pc = if index == 0 {
            frame.pc
        } else {
            unsafe { frame.pc.sub(1) }
        };
        Some((irep, pc))
    }
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
