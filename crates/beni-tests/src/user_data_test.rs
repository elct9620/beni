use crate::support::open_mrb;
use beni::{FromValue, Module, Mrb, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Debug, PartialEq)]
struct Realm {
    name: &'static str,
}

// Counts its own drops, so a test can see when the interpreter let go.
struct Tracked(Arc<AtomicUsize>);

impl Drop for Tracked {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn an_installed_value_reads_back_in_place() {
    let mut mrb = open_mrb();

    mrb.set_user_data(Realm { name: "game" })
        .expect("an empty slot accepts the value");

    assert_eq!(mrb.user_data::<Realm>(), Some(&Realm { name: "game" }));
}

#[test]
fn an_empty_slot_answers_nothing() {
    let mut mrb = open_mrb();

    assert_eq!(mrb.user_data::<Realm>(), None);
    assert_eq!(mrb.take_user_data::<Realm>(), None);
}

#[test]
fn a_read_naming_another_type_answers_nothing() {
    let mut mrb = open_mrb();
    mrb.set_user_data(Realm { name: "game" })
        .expect("an empty slot accepts the value");

    assert_eq!(mrb.user_data::<u32>(), None);
    assert_eq!(
        mrb.take_user_data::<u32>(),
        None,
        "a take naming another type answers nothing"
    );
    assert_eq!(
        mrb.user_data::<Realm>(),
        Some(&Realm { name: "game" }),
        "and leaves the held value in place"
    );
}

#[test]
fn installing_over_a_held_value_hands_the_offered_one_back() {
    let mut mrb = open_mrb();
    mrb.set_user_data(Realm { name: "first" })
        .expect("an empty slot accepts the value");

    let refused = mrb.set_user_data(Realm { name: "second" });

    assert_eq!(refused, Err(Realm { name: "second" }));
    assert_eq!(mrb.user_data::<Realm>(), Some(&Realm { name: "first" }));
}

#[test]
fn taking_empties_the_slot_so_a_new_value_installs() {
    let mut mrb = open_mrb();
    mrb.set_user_data(Realm { name: "first" })
        .expect("an empty slot accepts the value");

    let taken = mrb.take_user_data::<Realm>();
    mrb.set_user_data(7u32)
        .expect("a taken slot accepts a value of any type");

    assert_eq!(taken, Some(Realm { name: "first" }));
    assert_eq!(mrb.user_data::<u32>(), Some(&7));
}

#[test]
fn closing_the_interpreter_drops_the_held_value() {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut mrb = open_mrb();
    mrb.set_user_data(Tracked(Arc::clone(&drops)))
        .unwrap_or_else(|_| panic!("an empty slot accepts the value"));

    drop(mrb);

    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

// Answers the name of the realm installed on the interpreter it was
// called on, reached through the borrow the method frame hands it.
fn realm_name(mrb: &Mrb, _self: Value) -> Value {
    match mrb.user_data::<Realm>() {
        Some(realm) => mrb.str_new(realm.name.as_bytes()).as_value(),
        None => Value::nil(),
    }
}

#[test]
fn a_registered_method_reads_what_the_owner_installed() {
    let mut mrb = open_mrb();
    mrb.set_user_data(Realm { name: "game" })
        .expect("an empty slot accepts the value");
    mrb.object_class()
        .define_method(&mrb, c"realm_name", beni::method!(realm_name, 0))
        .expect("defining the method must succeed");

    let got = mrb
        .load_string(b"realm_name")
        .expect("the method must not raise");

    assert_eq!(String::from_value(got).as_deref(), Some("game"));
}
