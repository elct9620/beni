use beni::{Ccontext, DumpOptions, Error, FromValue, IntoValue, Mrb, Proc, Value};

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
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let block = proc_from(&mrb, b"proc { |x| x + 1 }");

    let got = block
        .call(&mrb, &[41i32.into_value(&mrb)])
        .expect("yielding a non-raising block must come back Ok");

    assert_eq!(i32::from_value(got), Some(42));
}

#[test]
fn call_surfaces_a_raised_exception_as_err() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
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
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    // A scalar carries no MRB_TT_PROC tag — the downcast rejects
    // instead of wrapping a value `mrb_yield_argv` would misread.
    assert!(Proc::from_value(42i32.into_value(&mrb)).is_none());
}

#[test]
fn as_value_round_trips_through_the_newtype() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let block = proc_from(&mrb, b"proc { 0 }");

    // The reified value is still Proc-tagged and downcasts back.
    let value: Value = block.as_value();
    assert!(Proc::from_value(value).is_some());
}

#[test]
fn a_dumped_program_loads_back_as_bytecode() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let cxt =
        Ccontext::new(&mrb, c"proc_test.rb").expect("allocating the compile context must succeed");
    let program = cxt
        .compile(b"$dumped = 41 + 1")
        .expect("plain source must compile");

    let bytes = program
        .dump(&mrb, DumpOptions::default())
        .expect("a Proc compiled from source must dump");

    assert_eq!(mrb.load_bytecode(&bytes), 0, "the dump must load back");
    assert_eq!(
        i32::from_value(mrb.gv_get(mrb.intern_cstr(c"$dumped"))),
        Some(42),
        "the loaded bytecode runs the program that was compiled"
    );
}

#[test]
fn asking_for_debug_info_carries_more_than_the_instructions() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
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

    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    // SAFETY: `stub` has the `mrb_func_t` ABI and is never called here;
    // `mrb_obj_value` boxes the RProc the constructor just returned.
    let value = unsafe {
        let raw = beni::sys::mrb_proc_new_cfunc(mrb.as_ptr(), stub);
        Value::from_raw(beni::sys::mrb_obj_value(raw.cast()))
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
