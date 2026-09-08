//! The compiler diagnostics beni reports a load's outcome in.
//!
//! mruby's parser records each diagnostic it produces — an error or a
//! warning — as a line, a column, and message text. `ParseMessage` is
//! the shape those cross into Rust in: `Ccontext::load_nstring`
//! returns one as `Error::Syntax` for source that does not parse, and
//! `Ccontext::warnings` answers the load's warnings as more of them.

use beni_sys as sys;

/// One compiler diagnostic's position and text.
///
/// The fields are private so the layout stays beni's to change; read
/// them through `line`, `column`, and `message`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseMessage {
    line: u16,
    column: i32,
    message: String,
}

impl ParseMessage {
    /// The 1-based source line the diagnostic points at, or 0 when the
    /// compiler recorded no diagnostic for the failure.
    #[inline]
    pub fn line(&self) -> u16 {
        self.line
    }

    /// The 0-based column the diagnostic points at, or 0 when the
    /// compiler recorded no diagnostic for the failure.
    #[inline]
    pub fn column(&self) -> i32 {
        self.column
    }

    /// The diagnostic text, empty when the compiler recorded no
    /// diagnostic for the failure.
    #[inline]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The message for a failure the compiler recorded no diagnostic
    /// for: zero position, empty text.
    pub(crate) fn unrecorded() -> Self {
        Self {
            line: 0,
            column: 0,
            message: String::new(),
        }
    }

    /// Copy every diagnostic the compiler wrote into `buffer`, in the
    /// order it wrote them.
    ///
    /// # Safety
    ///
    /// `buffer` must belong to a parser that has not been freed.
    pub(crate) unsafe fn recorded(buffer: &[sys::mrb_parser_message]) -> Vec<Self> {
        buffer
            .iter()
            .filter(|slot| !slot.message.is_null())
            // SAFETY: the slot is live by the caller's guarantee.
            .map(|slot| unsafe { Self::from_slot(slot) })
            .collect()
    }

    /// Copy the first diagnostic the compiler wrote into `buffer`.
    ///
    /// A parser counts a diagnostic before it writes one, so a failure
    /// arriving between the two leaves an earlier slot unwritten. An
    /// unwritten slot carries no position, so it is passed over rather
    /// than reported as one.
    ///
    /// # Safety
    ///
    /// `buffer` must belong to a parser that has not been freed.
    pub(crate) unsafe fn first_recorded(buffer: &[sys::mrb_parser_message]) -> Self {
        for slot in buffer {
            if !slot.message.is_null() {
                // SAFETY: the slot is live by the caller's guarantee.
                return unsafe { Self::from_slot(slot) };
            }
        }
        Self::unrecorded()
    }

    /// Copy one slot of a parser's diagnostic buffer.
    ///
    /// A slot whose text pointer is NULL was never written — the
    /// position beside it is not a position the compiler recorded — so
    /// it reads back as `unrecorded`.
    ///
    /// # Safety
    ///
    /// `slot` must point at a live `mrb_parser_message` belonging to a
    /// parser that has not been freed.
    pub(crate) unsafe fn from_slot(slot: *const sys::mrb_parser_message) -> Self {
        // SAFETY: the caller guarantees `slot` points at a live slot.
        let text = unsafe { (*slot).message };
        if text.is_null() {
            return Self::unrecorded();
        }
        Self {
            // SAFETY: as above.
            line: unsafe { (*slot).lineno },
            // SAFETY: as above.
            column: unsafe { (*slot).column },
            // SAFETY: `text` is non-NULL and NUL-terminated — the
            // parser copies the diagnostic with its terminator into
            // its own pool.
            message: unsafe { core::ffi::CStr::from_ptr(text) }
                .to_string_lossy()
                .into_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a diagnostic slot. `text` must outlive the slot's use.
    fn slot(lineno: u16, column: i32, text: Option<&std::ffi::CStr>) -> sys::mrb_parser_message {
        sys::mrb_parser_message {
            lineno,
            column,
            message: text.map_or(core::ptr::null_mut(), |t| t.as_ptr().cast_mut()),
        }
    }

    #[test]
    fn first_recorded_passes_over_a_slot_the_compiler_never_wrote() {
        // The parser counts a diagnostic before it writes one, so a
        // failure arriving between the two leaves slot 0 empty while
        // the diagnostic lands in slot 1.
        let buffer = [
            slot(0, 0, None),
            slot(7, 3, Some(c"memory allocation error")),
            slot(0, 0, None),
        ];

        let got = unsafe { ParseMessage::first_recorded(&buffer) };

        assert_eq!(got.line(), 7);
        assert_eq!(got.column(), 3);
        assert_eq!(got.message(), "memory allocation error");
    }

    #[test]
    fn first_recorded_reports_no_position_when_nothing_was_written() {
        let buffer = [slot(9, 9, None), slot(9, 9, None)];

        let got = unsafe { ParseMessage::first_recorded(&buffer) };

        assert_eq!(got, ParseMessage::unrecorded());
        assert_eq!(got.line(), 0);
        assert_eq!(got.column(), 0);
        assert_eq!(got.message(), "");
    }

    #[test]
    fn recorded_collects_every_written_slot_in_order() {
        let buffer = [
            slot(1, 0, Some(c"first")),
            slot(2, 4, Some(c"second")),
            slot(0, 0, None),
        ];

        let got = unsafe { ParseMessage::recorded(&buffer) };

        assert_eq!(got.len(), 2);
        assert_eq!(got[0].message(), "first");
        assert_eq!(got[1].line(), 2);
        assert_eq!(got[1].column(), 4);
    }
}
