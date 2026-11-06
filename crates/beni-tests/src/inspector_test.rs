use crate::support::open_mrb;
use beni::prelude::*;
use beni::{Ccontext, Error, FromValue, IntoValue, Mrb, Proc, RArray, RString, Value};

/// Run `source` under a file name, so its code carries line numbers.
fn run(mrb: &Mrb, source: &str) -> Result<Value, Error> {
    Ccontext::new(mrb, c"inspector_test.rb")
        .expect("allocating the compile context must succeed")
        .load_nstring(source.as_bytes())
}

fn current_depth(mrb: &Mrb, _self: Value) -> Result<i32, Error> {
    Ok(mrb.debug_inspector_current_depth() as i32)
}

#[test]
fn the_current_depth_counts_every_frame_of_the_running_fiber() {
    let mrb = open_mrb();
    mrb.class_object()
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
        mrb.eval::<beni::Value>("def deep(n)\n  n == 0 ? 0 : deep(n - 1)\nend\ndeep(300)\n")?;
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
    mrb.class_object()
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
    let x = mrb.intern(b"x").expect("a short name interns");
    mrb.debug_inspector_open(|frames| {
        let (_, local) = frames
            .frame_local_variables(1)
            .expect("the caller runs Ruby")
            .into_iter()
            .find(|&(name, _)| name == x)
            .expect("the caller declares `x`");
        block
            .expect("the test passes a block")
            .call::<_, beni::Value>(mrb, ())?;
        mrb.gc_start();
        Ok::<_, Error>(RString::from_value(local).is_some())
    })
}

#[test]
fn a_local_read_from_rust_stays_reachable_after_its_frame_lets_it_go() {
    let mrb = open_mrb();
    mrb.class_object()
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

/// The file names of the two frames beneath this method's, the first
/// read held across the second.
fn caller_paths(mrb: &Mrb, _self: Value) -> Result<RArray, Error> {
    let read = mrb.debug_inspector_open(|frames| {
        let first = frames.frame_path(1);
        let second = frames.frame_path(2);
        let ary = mrb.ary_new();
        for path in [first, second] {
            ary.push(mrb, path.as_deref().unwrap_or("").into_value(mrb))?;
        }
        Ok::<_, Error>(ary)
    })?;
    Ok(RArray::from_value(read).expect("the closure answers an Array"))
}

#[test]
fn a_file_name_read_stays_as_read_after_the_next() {
    let mrb = open_mrb();
    mrb.class_object()
        .define_method(&mrb, c"caller_paths", beni::method!(caller_paths, 0))
        .expect("registering the method succeeds");
    // Names of up to four letters or digits pack into the symbol itself,
    // the case a name read unpacks into a shared buffer.
    Ccontext::new(&mrb, c"ab")
        .expect("allocating the compile context must succeed")
        .load_nstring(b"def first_file\n  caller_paths\nend\n")
        .expect("the definition runs");
    let read = Ccontext::new(&mrb, c"cd")
        .expect("allocating the compile context must succeed")
        .load_nstring(b"first_file\n")
        .expect("the call runs");

    let paths = RArray::from_value(read).expect("the method answers an Array");
    let paths: Vec<String> = paths.to_vec(&mrb).expect("both paths are Strings");
    assert_eq!(paths, ["ab", "cd"]);
}
