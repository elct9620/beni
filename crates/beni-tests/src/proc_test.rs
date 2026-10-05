use crate::support::{hashes_on_the_heap, open_mrb};
use beni::prelude::*;
use beni::{Ccontext, DumpOptions, Error, FromValue, IntoValue, Mrb, Proc, RArray, Value};
use std::sync::atomic::{AtomicUsize, Ordering};

fn proc_from(mrb: &Mrb, src: &[u8]) -> Proc {
    let cxt =
        Ccontext::new(mrb, c"proc_test.rb").expect("allocating the compile context must succeed");
    let value = cxt
        .load_nstring(src)
        .expect("the test source must compile and run");
    assert!(
        mrb.pending_exc().is_nil(),
        "compiling the proc literal must not raise: {}",
        mrb.pending_exc().to_string(mrb)
    );
    Proc::from_value(value).expect("a proc literal carries MRB_TT_PROC")
}

#[test]
fn call_yields_to_the_block_and_returns_its_value() {
    let mrb = open_mrb();
    let block = proc_from(&mrb, b"proc { |x| x + 1 }");

    let got = block
        .call(&mrb, &[41i32.into_value(&mrb)])
        .expect("yielding a non-raising block must come back Ok");

    assert_eq!(i32::from_value(got), Some(42));
}

#[test]
fn call_surfaces_a_raised_exception_as_err() {
    let mrb = open_mrb();
    let block = proc_from(&mrb, b"proc { raise 'boom from block' }");

    let err = block
        .call(&mrb, &[])
        .expect_err("a raise inside the block must surface as Err");

    match err {
        Error::Exception(_) => assert!(err.message(&mrb).contains("boom from block")),
        other => panic!("a Ruby raise must surface as Error::Exception, got {other}"),
    }
    // The VM stays usable after the protected raise.
    let again = proc_from(&mrb, b"proc { 7 }");
    let got = again
        .call(&mrb, &[])
        .expect("the VM must survive the protected raise");
    assert_eq!(i32::from_value(got), Some(7));
}

#[test]
fn from_value_rejects_a_non_proc_value() {
    let mrb = open_mrb();

    // A scalar carries no MRB_TT_PROC tag — the downcast rejects
    // instead of wrapping a value `mrb_yield_argv` would misread.
    assert!(Proc::from_value(42i32.into_value(&mrb)).is_none());
}

#[test]
fn as_value_round_trips_through_the_newtype() {
    let mrb = open_mrb();
    let block = proc_from(&mrb, b"proc { 0 }");

    // The reified value is still Proc-tagged and downcasts back.
    let value: Value = block.as_value();
    assert!(Proc::from_value(value).is_some());
}

#[test]
fn a_dumped_program_loads_back_as_bytecode() {
    let mrb = open_mrb();
    let cxt =
        Ccontext::new(&mrb, c"proc_test.rb").expect("allocating the compile context must succeed");
    // The program answers a heap object, not an immediate: an
    // immediate result would read back the same whether or not the
    // load left its value arena-protected.
    let program = cxt
        .compile(b"$dumped = 41 + 1; 'answer'.dup")
        .expect("plain source must compile");

    let bytes = program
        .dump(&mrb, DumpOptions::default())
        .expect("a Proc compiled from source must dump");

    let got = mrb.load_bytecode(&bytes).expect("the dump must load back");

    // The result is the program's own value, and it is the caller's to
    // scope: a collection with the load's frame still live must not
    // reclaim it.
    mrb.full_gc();
    assert_eq!(
        String::from_value(got).as_deref(),
        Some("answer"),
        "the load yields the program's result, live after a collection"
    );
    assert_eq!(
        i32::from_value(mrb.gv_get(c"$dumped")),
        Some(42),
        "the loaded bytecode runs the program that was compiled"
    );
}

#[test]
fn asking_for_debug_info_carries_more_than_the_instructions() {
    let mrb = open_mrb();
    let cxt =
        Ccontext::new(&mrb, c"proc_test.rb").expect("allocating the compile context must succeed");
    let program = cxt
        .compile(b"a = 1\nb = 2\na + b\n")
        .expect("plain source must compile");

    let bare = program
        .dump(&mrb, DumpOptions::default())
        .expect("the bare dump must succeed");
    let annotated = program
        .dump(
            &mrb,
            DumpOptions {
                debug_info: true,
                locals: true,
            },
        )
        .expect("the annotated dump must succeed");

    assert!(
        annotated.len() > bare.len(),
        "line numbers and local names are carried only when asked for"
    );
}

#[test]
fn a_proc_backed_by_a_c_function_has_no_bytecode() {
    unsafe extern "C" fn stub(
        _mrb: *mut beni::sys::mrb_state,
        self_: beni::sys::mrb_value,
    ) -> beni::sys::mrb_value {
        self_
    }

    let mrb = open_mrb();
    // SAFETY: `stub` has the `mrb_func_t` ABI and is never called here;
    // `mrb_obj_value` boxes the RProc the constructor just returned.
    let value = unsafe {
        let raw = beni::sys::mrb_proc_new_cfunc(mrb.as_ptr(), stub);
        <Value as beni::sys::FromRawValue>::from_raw(beni::sys::mrb_obj_value(raw.cast()))
    };
    let cfunc = Proc::from_value(value).expect("the constructor answers a Proc-tagged value");

    let err = match cfunc.dump(&mrb, DumpOptions::default()) {
        Err(err) => err,
        Ok(_) => panic!("a Proc backed by a C function must not dump"),
    };

    assert!(
        err.message(&mrb).contains("C function"),
        "the error names why there is no bytecode"
    );
}

/// Bind `block` to the global `P` and run `src`, answering what it
/// evaluates to rendered by `inspect`.
fn run_with(mrb: &Mrb, block: Proc, src: &str) -> String {
    mrb.define_global_const("P", block)
        .expect("binding the proc must succeed");
    let value = mrb
        .load_string(src.as_bytes())
        .expect("the test source must run without raising");
    value.inspect(mrb)
}

fn add(_mrb: &Mrb, args: &[Value], _block: Option<Proc>) -> i32 {
    args.iter().filter_map(|arg| i32::from_value(*arg)).sum()
}

#[test]
fn a_proc_from_a_function_runs_it_over_the_call_arguments() {
    let mrb = open_mrb();
    let block = mrb.proc_new(add);

    assert_eq!(
        run_with(
            &mrb,
            block,
            "[P.call(1, 2), [[3, 4]].map { |a| P.call(*a) }]"
        ),
        "[3, [7]]"
    );
}

#[test]
fn a_proc_from_a_closure_keeps_its_state_across_calls() {
    let mrb = open_mrb();
    let mut count = 0;
    let block = mrb.proc_from_fn(move |_mrb, _args, _block| {
        count += 1;
        count
    });

    assert_eq!(
        run_with(&mrb, block, "[P.call, P.call, P.dup.call]"),
        "[1, 2, 3]"
    );
}

#[test]
fn a_rust_defined_proc_receives_the_call_block() {
    let mrb = open_mrb();
    let block = mrb.proc_from_fn(|mrb, args, block| match block {
        Some(block) => block.call(mrb, args),
        None => Ok(false.into_value(mrb)),
    });

    assert_eq!(
        run_with(&mrb, block, "[P.call(2) { |x| x * 10 }, P.call(2)]"),
        "[20, false]"
    );
}

fn collect(mrb: &Mrb, args: &[Value], _block: Option<Proc>) -> RArray {
    mrb.ary_new_from_values(args)
}

#[test]
fn a_rust_defined_proc_receives_the_call_keywords_as_its_last_argument() {
    let mrb = open_mrb();
    let block = mrb.proc_new(collect);

    assert_eq!(
        run_with(
            &mrb,
            block,
            "a = P.call(1, k: 2); b = P.call(1, **{}); [a.size, a.last[:k], b]"
        ),
        "[2, 2, [1]]"
    );
}

#[test]
fn a_rust_defined_proc_called_without_keywords_allocates_no_hash() {
    let mrb = open_mrb();
    mrb.define_global_const("P", mrb.proc_new(add))
        .expect("binding the proc must succeed");
    let calls = mrb
        .load_string(b"GC.disable; proc { 1000.times { P.call(1, 2) } }")
        .unwrap();
    let calls = Proc::from_value(calls).unwrap();
    let before = hashes_on_the_heap(&mrb);

    calls.call(&mrb, &[]).unwrap();

    assert_eq!(hashes_on_the_heap(&mrb), before);
}

fn refuse(mrb: &Mrb, _args: &[Value], _block: Option<Proc>) -> Result<Value, Error> {
    Err(Error::new(
        mrb,
        mrb.exc_get("ArgumentError").unwrap(),
        "refused by the body",
    ))
}

#[test]
fn an_err_from_the_body_raises_to_the_procs_caller() {
    let mrb = open_mrb();
    let block = mrb.proc_new(refuse);

    let rescued = run_with(
        &mrb,
        block,
        "begin; P.call; rescue ArgumentError => e; e.message; end",
    );
    let err = block
        .call(&mrb, &[])
        .expect_err("Proc::call answers the raise as Err");

    assert_eq!(rescued, "\"refused by the body\"");
    assert!(err.message(&mrb).contains("refused by the body"));
}

#[test]
fn a_panic_in_the_body_raises_runtime_error_to_the_procs_caller() {
    let mrb = open_mrb();
    let block = mrb.proc_from_fn(|_mrb, _args, _block| -> i32 { panic!("body panicked") });

    let rescued = run_with(
        &mrb,
        block,
        "begin; P.call; rescue RuntimeError => e; e.message; end",
    );

    assert!(rescued.contains("body panicked"), "got {rescued}");
    assert_eq!(
        run_with(&mrb, mrb.proc_new(add), "P.call(1)"),
        "1",
        "the interpreter stays usable"
    );
}

#[test]
fn a_closure_called_while_it_runs_raises_runtime_error_to_the_second_caller() {
    let mrb = open_mrb();
    let block = mrb.proc_from_fn(|mrb, args, _block| -> Result<Value, Error> {
        if args.is_empty() {
            return Ok(1.into_value(mrb));
        }
        // Re-enter through a copy, which shares this closure.
        let inner =
            mrb.load_string(b"begin; P.dup.call; rescue RuntimeError => e; e.class; end")?;
        Ok(inner)
    });

    assert_eq!(
        run_with(&mrb, block, "[P.call(:outer), P.call]"),
        "[RuntimeError, 1]"
    );
}

fn countdown(mrb: &Mrb, args: &[Value], _block: Option<Proc>) -> Result<Value, Error> {
    let n = i32::from_value(args[0]).unwrap();
    if n == 0 {
        return Ok(0.into_value(mrb));
    }
    let below = mrb.load_string(format!("P.call({})", n - 1).as_bytes())?;
    Ok((i32::from_value(below).unwrap() + n).into_value(mrb))
}

#[test]
fn a_proc_from_a_function_may_be_called_while_it_runs() {
    let mrb = open_mrb();
    let block = mrb.proc_new(countdown);

    assert_eq!(run_with(&mrb, block, "P.call(3)"), "6");
}

static CLOSURE_DROPS: AtomicUsize = AtomicUsize::new(0);

struct Counted;

impl Drop for Counted {
    fn drop(&mut self) {
        CLOSURE_DROPS.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn the_closure_is_dropped_once_the_proc_and_its_copies_are_reclaimed() {
    CLOSURE_DROPS.store(0, Ordering::SeqCst);
    let mrb = open_mrb();
    {
        let _scope = mrb.arena_scope();
        let counted = Counted;
        let block = mrb.proc_from_fn(move |_mrb, _args, _block| {
            let _ = &counted;
            true
        });
        mrb.define_global_const("COPY", block.as_value().funcall(&mrb, "dup", &[]).unwrap())
            .unwrap();
    }
    mrb.full_gc();
    let kept = CLOSURE_DROPS.load(Ordering::SeqCst);

    mrb.object_class().const_remove(&mrb, "COPY").unwrap();
    mrb.full_gc();

    assert_eq!(kept, 0, "a live copy keeps the closure");
    assert_eq!(
        CLOSURE_DROPS.load(Ordering::SeqCst),
        1,
        "the last reclaim drops it once"
    );
}

#[test]
fn closing_the_interpreter_drops_a_live_closure() {
    let drops = std::sync::Arc::new(AtomicUsize::new(0));
    struct Flag(std::sync::Arc<AtomicUsize>);
    impl Drop for Flag {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let flag = Flag(drops.clone());
    let mrb = open_mrb();
    let block = mrb.proc_from_fn(move |_mrb, _args, _block| {
        let _ = &flag;
        true
    });
    mrb.define_global_const("P", block).unwrap();

    drop(mrb);

    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[test]
fn a_rust_defined_proc_has_no_dump() {
    let mrb = open_mrb();
    let block = mrb.proc_new(add);

    assert!(block.dump(&mrb, DumpOptions::default()).is_err());
}

#[test]
fn a_rust_defined_proc_runs_as_a_method_body_and_under_instance_exec() {
    let mrb = open_mrb();
    let block = mrb.proc_new(add);

    let got = run_with(
        &mrb,
        block,
        "class Adder; define_method(:sum, &P); end; [Adder.new.sum(1, 2), 1.instance_exec(3, 4, &P)]",
    );

    assert_eq!(got, "[3, 7]");
}
