use crate::support::open_mrb;
use beni::debug::TraceArg;
use beni::prelude::*;
use beni::{Ccontext, Error, FromValue, Id, Mrb, Proc, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Run `source` under the file name the tests read back.
fn run(mrb: &Mrb, source: &str) -> Result<Value, Error> {
    Ccontext::new(mrb, c"debug_test.rb")
        .expect("allocating the compile context must succeed")
        .load_nstring(source.as_bytes())
}

fn id(mrb: &Mrb, name: &str) -> Id {
    mrb.intern(name.as_bytes()).expect("a short name interns")
}

// Counts its own drops, so a test can see when the interpreter let go.
struct Tracked(Arc<AtomicUsize>);

impl Drop for Tracked {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn yield_to(mrb: &Mrb, _self: Value, block: Option<Proc>) -> Result<Value, Error> {
    block.expect("the test passes a block").call(mrb, &[])
}

#[test]
fn the_hook_reads_each_line_and_the_file_running_it() {
    let mut mrb = open_mrb();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    mrb.set_code_fetch_hook(move |arg: TraceArg<'_>| {
        let place = (
            arg.path().map(|path| path.into_owned()),
            arg.lineno().map(|line| line.get()),
        );
        sink.lock().unwrap().push(place);
    });

    run(&mrb, "a = 1\nb = 2\nc = a + b\n").expect("the source runs");

    let seen = seen.lock().unwrap();
    for line in 1..=3 {
        assert!(
            seen.contains(&(Some("debug_test.rb".to_owned()), Some(line))),
            "the hook runs before line {line}"
        );
    }
}

#[test]
fn a_removed_hook_is_called_no_more() {
    let mut mrb = open_mrb();
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    mrb.set_code_fetch_hook(move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    run(&mrb, "1 + 1").expect("the source runs");
    let before = calls.load(Ordering::SeqCst);
    assert!(before > 0, "an installed hook is called");

    mrb.remove_code_fetch_hook();
    run(&mrb, "1 + 1").expect("the source runs");

    assert_eq!(calls.load(Ordering::SeqCst), before);
}

#[test]
fn installing_a_hook_drops_the_one_before_and_closing_drops_the_last() {
    let mut mrb = open_mrb();
    let drops = Arc::new(AtomicUsize::new(0));
    let first = Tracked(Arc::clone(&drops));
    mrb.set_code_fetch_hook(move |_| {
        let _ = &first;
    });
    let second = Tracked(Arc::clone(&drops));
    mrb.set_code_fetch_hook(move |_| {
        let _ = &second;
    });
    assert_eq!(
        drops.load(Ordering::SeqCst),
        1,
        "the replaced hook is dropped"
    );

    drop(mrb);

    assert_eq!(
        drops.load(Ordering::SeqCst),
        2,
        "closing drops the installed hook"
    );
}

/// What a hook saw of the frames at one instruction.
#[derive(Debug, Default)]
struct Frames {
    running_method: Option<Id>,
    caller_method: Option<Id>,
    caller_path: Option<String>,
    caller_line: Option<usize>,
    running_depth: Option<usize>,
    caller_depth: Option<usize>,
    outermost_method: Option<Id>,
    past_outermost: Option<usize>,
    locals: Vec<(Id, Option<i32>)>,
}

/// Run `source` with a hook that records the frames once, at the first
/// instruction of `line`.
fn frames_at(mrb: &mut Mrb, source: &str, line: usize) -> Frames {
    let recorded = Arc::new(Mutex::new(None));
    let sink = Arc::clone(&recorded);
    mrb.set_code_fetch_hook(move |arg: TraceArg<'_>| {
        let mut sink = sink.lock().unwrap();
        if sink.is_some() || arg.lineno().map(|l| l.get()) != Some(line) {
            return;
        }
        let frames = arg.debug_inspector();
        let running_depth = frames.frame_depth(0);
        *sink = Some(Frames {
            running_method: arg.method_id(),
            caller_method: frames.frame_method_id(1),
            caller_path: frames.frame_path(1).map(|path| path.into_owned()),
            caller_line: frames.frame_lineno(1).map(|line| line.get()),
            running_depth,
            caller_depth: frames.frame_depth(1),
            outermost_method: running_depth.and_then(|depth| frames.frame_method_id(depth)),
            past_outermost: running_depth.and_then(|depth| frames.frame_depth(depth + 1)),
            locals: frames
                .frame_local_variables(0)
                .expect("the running frame exists")
                .into_iter()
                .map(|(name, value)| (name, i32::from_value(value)))
                .collect(),
        });
    });
    run(mrb, source).expect("the source runs");
    mrb.remove_code_fetch_hook();
    let frames = recorded.lock().unwrap().take();
    frames.expect("the hook reached the line")
}

#[test]
fn the_inspector_walks_from_the_running_frame_to_its_callers() {
    let mut mrb = open_mrb();
    let (inner, outer, x) = (id(&mrb, "inner"), id(&mrb, "outer"), id(&mrb, "x"));
    let source = "def inner\n  x = 5\n  x + 1\nend\ndef outer\n  inner\n  9\nend\nouter\n";

    let frames = frames_at(&mut mrb, source, 3);

    assert_eq!(frames.running_method, Some(inner));
    assert_eq!(frames.caller_method, Some(outer));
    assert_eq!(frames.caller_path.as_deref(), Some("debug_test.rb"));
    assert_eq!(
        frames.caller_line,
        Some(6),
        "a caller stands at its pending call"
    );
    assert_eq!(
        frames.running_depth,
        frames.caller_depth.map(|depth| depth + 1)
    );
    assert_eq!(
        frames.outermost_method, None,
        "the outermost frame is the top level"
    );
    assert_eq!(frames.past_outermost, None);
    assert_eq!(frames.locals, vec![(x, Some(5))]);
}

#[test]
fn a_frame_running_rust_has_no_place_and_no_locals() {
    let mut mrb = open_mrb();
    mrb.object_class()
        .define_method(&mrb, c"yield_to", beni::method!(yield_to, 0, &))
        .expect("registering the method succeeds");
    let (yield_to, y) = (id(&mrb, "yield_to"), id(&mrb, "y"));
    let source = "yield_to do\n  y = 7\n  y\nend\n";

    let frames = frames_at(&mut mrb, source, 3);

    assert_eq!(frames.caller_method, Some(yield_to));
    assert_eq!(frames.caller_path, None);
    assert_eq!(frames.caller_line, None);
    assert_eq!(
        frames.locals,
        vec![(y, Some(7))],
        "the block reads its own locals"
    );
}

#[test]
fn an_err_at_the_first_instruction_of_a_rescued_range_is_rescued() {
    let mut mrb = open_mrb();
    let raised = Arc::new(AtomicUsize::new(0));
    let once = Arc::clone(&raised);
    mrb.set_code_fetch_hook(move |arg: TraceArg<'_>| {
        if arg.lineno().map(|l| l.get()) == Some(2) && once.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(Error::Panic("stopped by the hook".to_owned()));
        }
        Ok(())
    });

    let rescued = run(&mrb, "r = begin\n  1\nrescue => e\n  e.message\nend\nr\n")
        .expect("the running code rescues the raise");

    assert_eq!(
        String::from_value(rescued).as_deref(),
        Some("stopped by the hook")
    );
}

#[test]
fn a_panic_in_the_hook_is_raised_in_place_of_the_instruction() {
    let mut mrb = open_mrb();
    let raised = Arc::new(AtomicUsize::new(0));
    let once = Arc::clone(&raised);
    mrb.set_code_fetch_hook(move |arg: TraceArg<'_>| {
        if arg.lineno().map(|l| l.get()) == Some(2) && once.fetch_add(1, Ordering::SeqCst) == 0 {
            panic!("the hook panicked");
        }
    });

    let rescued = run(&mrb, "r = begin\n  1\nrescue => e\n  e.message\nend\nr\n")
        .expect("the running code rescues the raise");

    let message = String::from_value(rescued).expect("the rescue answers the message");
    assert!(message.contains("the hook panicked"), "{message}");
}

#[test]
fn a_raise_from_the_hook_is_backtraced_to_the_instruction_it_replaced() {
    let mut mrb = open_mrb();
    let raised = Arc::new(AtomicUsize::new(0));
    let once = Arc::clone(&raised);
    mrb.set_code_fetch_hook(move |arg: TraceArg<'_>| {
        if arg.lineno().map(|l| l.get()) == Some(3) && once.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(Error::Panic("stopped by the hook".to_owned()));
        }
        Ok(())
    });

    let escaped =
        run(&mrb, "a = 1\nb = a + 1\nc = b + 1\n").expect_err("nothing rescues the raise");

    let backtrace = escaped.backtrace(&mrb);
    assert!(backtrace[0].ends_with("debug_test.rb:3"), "{backtrace:?}");
}
