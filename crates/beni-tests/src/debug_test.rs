use crate::support::open_mrb;
use beni::debug::TraceArg;
use beni::prelude::*;
use beni::{
    Ccontext, DumpOptions, Error, FromValue, Id, IntoValue, Mrb, Proc, RArray, RString, Value,
};
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
fn bytecode_dumped_without_its_lines_has_no_place_and_no_locals() {
    let mut mrb = open_mrb();
    let bytes = Ccontext::new(&mrb, c"debug_test.rb")
        .expect("allocating the compile context must succeed")
        .compile(b"a = 1\nb = a + 1\n")
        .expect("the source compiles")
        .dump(&mrb, DumpOptions::default())
        .expect("a compiled proc dumps");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    mrb.set_code_fetch_hook(move |arg: TraceArg<'_>| {
        let locals = arg
            .debug_inspector()
            .frame_local_variables(0)
            .map_or(0, |locals| locals.len());
        sink.lock()
            .unwrap()
            .push((arg.path().is_some(), arg.lineno().is_some(), locals));
    });

    // SAFETY: the bytes are the dump this interpreter just produced.
    unsafe { mrb.load_irep(&bytes) }.expect("the bytecode runs");

    let seen = seen.lock().unwrap();
    assert!(!seen.is_empty(), "the hook runs");
    assert!(
        seen.iter().all(|&place| place == (false, false, 0)),
        "{seen:?}"
    );
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

#[test]
fn the_hook_views_copy_as_magnuss_do() {
    fn copied<T: Copy>() {}
    copied::<beni::debug::TraceArg<'static>>();
    copied::<beni::debug::DebugInspector<'static>>();
}

/// Run `source` with a hook that answers `read` once, at the first
/// instruction of `line`.
fn read_at<T: Send + 'static>(
    mrb: &mut Mrb,
    source: &str,
    line: usize,
    read: impl Fn(TraceArg<'_>) -> T + Send + 'static,
) -> T {
    let recorded = Arc::new(Mutex::new(None));
    let sink = Arc::clone(&recorded);
    mrb.set_code_fetch_hook(move |arg: TraceArg<'_>| {
        let mut sink = sink.lock().unwrap();
        if sink.is_none() && arg.lineno().map(|l| l.get()) == Some(line) {
            *sink = Some(read(arg));
        }
    });
    run(mrb, source).expect("the source runs");
    mrb.remove_code_fetch_hook();
    let read = recorded.lock().unwrap().take();
    read.expect("the hook reached the line")
}

/// A class or module read as whether it is a module and its identity.
fn kind_and_id(class: Option<beni::ClassOrModule>) -> Option<(bool, i64)> {
    class.map(|class| {
        (
            matches!(class, beni::ClassOrModule::Module(_)),
            class.as_value().object_id(),
        )
    })
}

fn object_id(mrb: &Mrb, source: &str) -> i64 {
    run(mrb, source).expect("the source runs").object_id()
}

#[test]
fn the_hook_reads_self_and_the_class_defining_the_method() {
    let mut mrb = open_mrb();
    let source = "class Greeter\n  def hello\n    1\n  end\nend\n$g = Greeter.new\n$g.hello\n";

    let (running, caller, defined, caller_defined) = read_at(&mut mrb, source, 3, |arg| {
        let frames = arg.debug_inspector();
        (
            arg.tracearg_self().object_id(),
            frames.frame_self_get(1).map(|v| v.object_id()),
            kind_and_id(arg.defined_class()),
            kind_and_id(frames.frame_class_get(1)),
        )
    });

    assert_eq!(running, object_id(&mrb, "$g"));
    assert_eq!(caller, Some(object_id(&mrb, "self")));
    assert_eq!(defined, Some((false, object_id(&mrb, "Greeter"))));
    assert_eq!(caller_defined, None, "the top level has no defining class");
}

#[test]
fn a_module_method_answers_its_module_and_a_prepended_class_itself() {
    let source = "module Helper\n  def help\n    2\n  end\nend\nmodule Loud\nend\n\
                  class Shout\n  prepend Loud\n  include Helper\n  def say\n    3\n  end\nend\n\
                  s = Shout.new\ns.help\ns.say\n";
    let mut mrb = open_mrb();

    let help = read_at(&mut mrb, source, 3, |arg| kind_and_id(arg.defined_class()));
    let say = read_at(&mut mrb, source, 12, |arg| kind_and_id(arg.defined_class()));

    assert_eq!(help, Some((true, object_id(&mrb, "Helper"))));
    assert_eq!(say, Some((false, object_id(&mrb, "Shout"))));
}

#[test]
fn a_rust_frame_answers_the_module_its_method_was_found_in() {
    let mut mrb = open_mrb();
    let helper = mrb
        .define_module(c"BeniDebugHelper")
        .expect("defining the module succeeds");
    helper
        .define_method(&mrb, c"yield_to", beni::method!(yield_to, 0, &))
        .expect("registering the method succeeds");
    let source = "class Holder\n  include BeniDebugHelper\nend\nHolder.new.yield_to do\n  1\nend\n";

    let caller = read_at(&mut mrb, source, 5, |arg| {
        kind_and_id(arg.debug_inspector().frame_class_get(1))
    });

    assert_eq!(caller, Some((true, object_id(&mrb, "BeniDebugHelper"))));
}

fn current_depth(mrb: &Mrb, _self: Value) -> Result<i32, Error> {
    Ok(mrb.debug_inspector_current_depth() as i32)
}

#[test]
fn the_current_depth_counts_every_frame_of_the_running_fiber() {
    let mrb = open_mrb();
    mrb.object_class()
        .define_method(&mrb, c"depth", beni::method!(current_depth, 0))
        .expect("registering the method succeeds");

    let top = run(&mrb, "depth").expect("the source runs");
    let nested = run(
        &mrb,
        "def foo\n  bar\nend\ndef bar\n  baz\nend\ndef baz\n  depth\nend\nfoo\n",
    )
    .expect("the source runs");

    assert_eq!(i32::from_value(top), Some(2), "the top level and `depth`");
    assert_eq!(i32::from_value(nested), Some(5));
}

/// The line and method of the frame that called this method, read once
/// before and once after a call deep enough to move the frame stack.
fn caller_twice(mrb: &Mrb, _self: Value) -> Result<RArray, Error> {
    let read = mrb.debug_inspector_open(|frames| {
        let place = |frames: beni::debug::DebugInspector<'_>| {
            (
                frames.frame_lineno(1).map_or(0, |line| line.get() as i32),
                frames.frame_method_id(1),
            )
        };
        let before = place(frames);
        mrb.load_string(b"def deep(n)\n  n == 0 ? 0 : deep(n - 1)\nend\ndeep(300)\n")?;
        let after = place(frames);
        let ary = mrb.ary_new();
        for (line, method) in [before, after] {
            ary.push(mrb, line.into_value(mrb))?;
            ary.push(
                mrb,
                method.expect("the caller runs a method").into_value(mrb),
            )?;
        }
        Ok::<_, Error>(ary)
    })?;
    Ok(RArray::from_value(read).expect("the closure answers an Array"))
}

#[test]
fn an_inspector_opened_from_rust_reads_its_callers_across_a_moved_frame_stack() {
    let mrb = open_mrb();
    mrb.object_class()
        .define_method(&mrb, c"caller_twice", beni::method!(caller_twice, 0))
        .expect("registering the method succeeds");

    let read =
        run(&mrb, "def outer\n  caller_twice\nend\nouter.inspect\n").expect("the source runs");

    assert_eq!(
        RString::from_value(read)
            .expect("inspect answers a String")
            .to_string(&mrb)
            .expect("renders"),
        "[2, :outer, 2, :outer]"
    );
}

/// Read the caller's local `x`, let the block drop the caller's
/// reference to it, collect, and answer whether the read value is
/// still a String. A collected object reads back with the free tag.
fn local_survives_its_reassignment(
    mrb: &Mrb,
    _self: Value,
    block: Option<Proc>,
) -> Result<Value, Error> {
    let x = id(mrb, "x");
    mrb.debug_inspector_open(|frames| {
        let (_, local) = frames
            .frame_local_variables(1)
            .expect("the caller runs Ruby")
            .into_iter()
            .find(|&(name, _)| name == x)
            .expect("the caller declares `x`");
        block.expect("the test passes a block").call(mrb, &[])?;
        mrb.full_gc();
        Ok::<_, Error>(RString::from_value(local).is_some())
    })
}

#[test]
fn a_local_read_from_rust_stays_reachable_after_its_frame_lets_it_go() {
    let mrb = open_mrb();
    mrb.object_class()
        .define_method(
            &mrb,
            c"local_survives_its_reassignment",
            beni::method!(local_survives_its_reassignment, 0, &),
        )
        .expect("registering the method succeeds");

    let survived = run(
        &mrb,
        "def outer\n  x = 'read' * 64\n  local_survives_its_reassignment { x = nil }\nend\nouter\n",
    )
    .expect("the source runs");

    assert_eq!(bool::from_value(survived), Some(true));
}

#[test]
fn a_hook_in_a_fiber_reads_only_that_fibers_frames() {
    let mut mrb = open_mrb();
    let source = "def wrap\n  Fiber.new do\n    y = 3\n    y\n  end.resume\nend\nwrap\n";

    let (depth, past) = read_at(&mut mrb, source, 4, |arg| {
        let frames = arg.debug_inspector();
        (frames.frame_depth(0), frames.frame_depth(1))
    });

    assert_eq!(depth, Some(0), "the fiber's block is its outermost frame");
    assert_eq!(past, None, "the frames that resumed it are another fiber's");
}

#[test]
fn the_running_frames_line_is_the_instructions() {
    let mut mrb = open_mrb();
    let agree = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&agree);
    mrb.set_code_fetch_hook(move |arg: TraceArg<'_>| {
        let running = arg.debug_inspector().frame_lineno(0);
        sink.lock().unwrap().push(running == arg.lineno());
    });

    run(&mrb, "a = 1\nb = [a, 2].map { |n| n * 2 }\nc = b.sum\n").expect("the source runs");
    mrb.remove_code_fetch_hook();

    let agree = agree.lock().unwrap();
    assert!(!agree.is_empty());
    assert!(agree.iter().all(|&same| same));
}

/// Runs Ruby as the interpreter closes, as an embedder's exit handler may.
unsafe extern "C" fn run_ruby_on_close(mrb: *mut beni::sys::mrb_state) {
    // SAFETY: mruby calls an exit handler with its live state.
    unsafe { beni::sys::mrb_load_string(mrb, c"[1, 2].map { |n| n + 1 }".as_ptr()) };
}

#[test]
fn closing_calls_the_hook_for_nothing_the_close_runs() {
    let mut mrb = open_mrb();
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    mrb.set_code_fetch_hook(move |_: TraceArg<'_>| {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    // SAFETY: `mrb` is live, and the handler runs only Ruby.
    unsafe { beni::sys::mrb_state_atexit(mrb.as_ptr(), Some(run_ruby_on_close)) };
    run(&mrb, "1 + 1").expect("the source runs");
    let before = calls.load(Ordering::SeqCst);

    drop(mrb);

    assert!(before > 0);
    assert_eq!(calls.load(Ordering::SeqCst), before);
}
