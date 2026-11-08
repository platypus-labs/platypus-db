use mongodb::bson::{Bson, doc};

use super::*;
use crate::shell::{CollectionOp, Command, parse, split};

const ID: &str = "65a1b2c3d4e5f60718293a4b";

fn names(items: &[&str]) -> Vec<String> {
    items.iter().map(|c| (*c).to_owned()).collect()
}

#[allow(clippy::unnecessary_wraps)]
fn some(text: &str) -> Option<String> {
    Some(text.to_owned())
}

fn edits<'a>(columns: &'a [String], types: &'a [String]) -> Edits<'a> {
    Edits {
        db: None,
        collection: "orders",
        columns,
        types,
        updates: Vec::new(),
        inserts: Vec::new(),
        deletes: Vec::new(),
    }
}

/// One update of the first column set to `new`, on a row whose `_id` is [`ID`].
fn updated(column: &str, ty: &str, old: Option<&str>, new: Option<&str>) -> Result<Script, String> {
    let columns = names(&["_id", column]);
    let types = names(&["objectId", ty]);
    let mut e = edits(&columns, &types);
    e.updates.push(Update {
        loaded: vec![some(ID), old.map(str::to_owned)],
        set: vec![(1, new.map(str::to_owned))],
    });
    script(&e)
}

/// The values an `updateOne` sets: those of its `$set`, or of the `$set` stage of its pipeline,
/// where a literal is its value and a number kept in the stored type is its default.
fn set_of(command: &str) -> Document {
    let Ok(Command::Collection {
        op: CollectionOp::Update { update, many, .. },
        ..
    }) = parse(command)
    else {
        panic!("not an updateOne: {command}");
    };
    assert!(!many);
    let set = match update {
        Bson::Document(update) => return update.get_document("$set").unwrap().clone(),
        Bson::Array(stages) => {
            assert_eq!(stages.len(), 1, "{command}");
            stages[0]
                .as_document()
                .and_then(|s| s.get_document("$set").ok())
                .unwrap()
                .clone()
        }
        other => panic!("{other}"),
    };
    set.into_iter()
        .map(|(name, value)| {
            let Bson::Document(expression) = value else {
                panic!("{name}: not an expression in {command}");
            };
            let value = match expression.get_document("$switch") {
                Ok(switch) => switch.get("default").unwrap().clone(),
                Err(_) => expression.get("$literal").unwrap().clone(),
            };
            (name, value)
        })
        .collect()
}

#[test]
fn updates_a_document_by_id_and_undoes() {
    let columns = names(&["_id", "note", "paid"]);
    let types = names(&["objectId", "string", "bool"]);
    let mut e = edits(&columns, &types);
    e.updates.push(Update {
        loaded: vec![some(ID), some("hi"), some("false")],
        set: vec![(1, None), (2, some("TRUE"))],
    });
    let s = script(&e).unwrap();
    assert_eq!(
        s.commands,
        [format!(
            "db.orders.updateOne({{ _id: ObjectId(\"{ID}\") }}, {{ $set: {{ note: null, paid: true }} }})"
        )]
    );
    assert_eq!(
        s.undo,
        [format!(
            "db.orders.updateOne({{ _id: ObjectId(\"{ID}\") }}, {{ $set: {{ note: \"hi\", paid: false }} }})"
        )]
    );
}

#[test]
fn a_changed_number_keeps_the_type_the_document_stores() {
    let columns = names(&["_id", "total", "note", "tags"]);
    let types = names(&["objectId", "long", "string", "document"]);
    let mut e = edits(&columns, &types);
    e.updates.push(Update {
        loaded: vec![some(ID), some("5"), some("hi"), some("{\"a\":1}")],
        set: vec![
            (1, some("7")),
            (2, some("$not an operator")),
            (3, some("{\"$price\": 2}")),
        ],
    });
    let s = script(&e).unwrap();
    let total = "{ $switch: { branches: [\
        { case: { $eq: [{ $type: \"$total\" }, \"int\"] }, then: 7 }, \
        { case: { $eq: [{ $type: \"$total\" }, \"long\"] }, then: NumberLong(7) }, \
        { case: { $eq: [{ $type: \"$total\" }, \"double\"] }, then: 7.0 }, \
        { case: { $eq: [{ $type: \"$total\" }, \"decimal\"] }, then: NumberDecimal(\"7\") }], \
        default: NumberLong(7) } }";
    assert_eq!(
        s.commands,
        [format!(
            "db.orders.updateOne({{ _id: ObjectId(\"{ID}\") }}, [{{ $set: {{ total: {total}, \
             note: {{ $literal: \"$not an operator\" }}, tags: {{ $literal: {{ $price: 2 }} }} }} }}])"
        )]
    );
    let set = set_of(&s.commands[0]);
    assert_eq!(set.get("total"), Some(&Bson::Int64(7)));
    assert_eq!(set.get_str("note"), Ok("$not an operator"));
    assert_eq!(set.get_document("tags"), Ok(&doc! { "$price": 2 }));
    // The undo puts the number back by the same switch; a whole number has every type.
    assert_eq!(s.undo.len(), 1, "{:?}", s.undo);
    assert!(
        s.undo[0].contains("default: NumberLong(5)"),
        "{}",
        s.undo[0]
    );
    assert_eq!(set_of(&s.undo[0]).get_str("note"), Ok("hi"));

    // 3.5 is no int or long: such a field becomes a double, which the undo says.
    let s = updated("qty", "int", Some("2"), Some("3.5")).unwrap();
    let command = &s.commands[0];
    assert!(
        !command.contains("\"int\"] }") && !command.contains("\"long\"] }"),
        "{command}"
    );
    assert!(command.contains("default: 3.5"), "{command}");
    assert!(
        s.undo[0].starts_with("// _id") && s.undo[0].contains("qty (not an exact int or long)"),
        "{:?}",
        s.undo
    );
    // Beyond 2^53 a long has no double branch.
    let s = updated("n", "long", None, Some("9007199254740993")).unwrap();
    assert!(
        !s.commands[0].contains("\"double\"] }"),
        "{}",
        s.commands[0]
    );
    assert!(s.commands[0].contains("default: NumberLong(\"9007199254740993\")"));
    // A NULL number is a literal null.
    let s = updated("n", "long", Some("5"), None).unwrap();
    assert!(
        s.commands[0].ends_with("{ $set: { n: null } })"),
        "{}",
        s.commands[0]
    );
    assert!(s.undo[0].contains("$switch"), "{}", s.undo[0]);
}

#[test]
fn writes_every_type_by_its_column() {
    let cases = [
        ("string", " a b ", Bson::String(" a b ".into())),
        ("bool", "True", Bson::Boolean(true)),
        ("int", "-12", Bson::Int32(-12)),
        (
            "long",
            "9007199254740993",
            Bson::Int64(9_007_199_254_740_993),
        ),
        ("double", "3", Bson::Double(3.0)),
        ("double", "1.5e-7", Bson::Double(1.5e-7)),
        ("decimal", "1.10", Bson::Decimal128("1.10".parse().unwrap())),
        (
            "objectId",
            ID,
            Bson::ObjectId(ObjectId::parse_str(ID).unwrap()),
        ),
        (
            "date",
            "2024-01-02 10:30:00.123+00:00",
            Bson::DateTime(
                mongodb::bson::DateTime::parse_rfc3339_str("2024-01-02T10:30:00.123Z").unwrap(),
            ),
        ),
        (
            "date",
            "2024-01-02",
            Bson::DateTime(
                mongodb::bson::DateTime::parse_rfc3339_str("2024-01-02T00:00:00Z").unwrap(),
            ),
        ),
        (
            "document",
            "{\"a\":1,\"b\":{\"$oid\":\"65a1b2c3d4e5f60718293a4b\"}}",
            Bson::Document(doc! { "a": 1, "b": ObjectId::parse_str(ID).unwrap() }),
        ),
        (
            "array",
            "[1, \"x\", {\"$date\":\"2024-01-02T00:00:00Z\"}]",
            Bson::Array(vec![
                Bson::Int32(1),
                Bson::String("x".into()),
                Bson::DateTime(
                    mongodb::bson::DateTime::parse_rfc3339_str("2024-01-02T00:00:00Z").unwrap(),
                ),
            ]),
        ),
    ];
    for (ty, text, expected) in cases {
        let s = updated("f", ty, None, Some(text)).unwrap_or_else(|e| panic!("{ty} {text}: {e}"));
        // What the statement says, read back by the shell's own parser, is the value.
        assert_eq!(
            set_of(&s.commands[0]).get("f"),
            Some(&expected),
            "{ty} {text}"
        );
    }
}

#[test]
fn a_document_keeps_its_longs_and_doubles_through_the_grid() {
    // The cell of a document is its relaxed Extended JSON, as the grid shows it.
    let nested = doc! {
        "big": 9_007_199_254_740_993_i64,
        "wide": 3_000_000_000_i64,
        "whole": 5.0,
        "huge": 1e21,
        "minus_zero": -0.0,
        "nan": f64::NAN,
        "at": mongodb::bson::DateTime::from_millis(1_704_191_400_123),
        "list": [1, 2.5, -2_147_483_649_i64],
    };
    let text = crate::docs::text(&Bson::Document(nested.clone())).unwrap();
    let s = updated("f", "document", None, Some(&text)).unwrap();
    let Some(Bson::Document(written)) = set_of(&s.commands[0]).get("f").cloned() else {
        panic!("{}", s.commands[0]);
    };
    for (key, value) in &nested {
        match (value, written.get(key)) {
            (Bson::Double(a), Some(Bson::Double(b))) if a.is_nan() => assert!(b.is_nan()),
            (Bson::Double(a), Some(Bson::Double(b))) => {
                assert_eq!(a.to_bits(), b.to_bits(), "{key}");
            }
            (a, b) => assert_eq!(Some(a), b, "{key}: {text}"),
        }
    }
}

#[test]
fn a_whole_double_is_not_read_back_as_an_int() {
    let s = updated("f", "double", None, Some("2")).unwrap();
    assert!(
        s.commands[0].contains("default: 2.0 }"),
        "{}",
        s.commands[0]
    );
    assert_eq!(set_of(&s.commands[0]).get("f"), Some(&Bson::Double(2.0)));
    for (text, check) in [
        ("NaN", f64::is_nan as fn(f64) -> bool),
        ("inf", f64::is_infinite),
        ("-inf", f64::is_infinite),
    ] {
        let s = updated("f", "double", None, Some(text)).unwrap();
        let Some(Bson::Double(d)) = set_of(&s.commands[0]).get("f").cloned() else {
            panic!()
        };
        assert!(check(d), "{text}");
    }
}

#[test]
fn a_null_cell_sets_null_and_the_undo_says_it_may_have_been_missing() {
    let s = updated("note", "string", Some("hi"), None).unwrap();
    assert!(s.commands[0].ends_with("{ $set: { note: null } })"));
    assert_eq!(s.undo.len(), 1);
    let s = updated("note", "string", None, Some("x")).unwrap();
    assert_eq!(s.undo.len(), 2);
    assert!(s.undo[0].starts_with("//"), "{:?}", s.undo);
    assert!(s.undo[0].contains("note"));
    assert!(s.undo[1].contains("note: null"));
}

#[test]
fn refuses_values_that_do_not_fit() {
    for (ty, text, message) in [
        ("int", "abc", "invalid number"),
        ("long", "1.5.2", "invalid number"),
        ("double", "abc", "invalid number"),
        ("double", "0x10", "invalid number"),
        ("bool", "yes", "true or false"),
        ("objectId", "xyz", "ObjectId"),
        ("date", "yesterday", "invalid date"),
        ("decimal", "1,5", "invalid number"),
        ("document", "[1]", "not a document"),
        ("document", "{ a: ", "not a document"),
        ("array", "{\"a\":1}", "not an array"),
        ("array", "5", "not an array"),
    ] {
        let e = updated("f", ty, None, Some(text)).unwrap_err();
        assert!(
            e.contains(message) && e.contains("column f"),
            "{ty} {text}: {e}"
        );
    }
}

#[test]
fn refuses_columns_it_cannot_write_back_exactly() {
    for ty in ["mixed", "binData", "regex", "timestamp", ""] {
        for new in [Some("1"), None] {
            let e = updated("f", ty, Some("0"), new).unwrap_err();
            assert!(e.starts_with("column f: "), "{ty}: {e}");
        }
    }
    assert!(
        updated("f", "mixed", None, Some("1"))
            .unwrap_err()
            .contains("several types")
    );
}

#[test]
fn refuses_field_names_that_set_reads_as_paths_or_operators() {
    for name in ["a.b", "$a", ""] {
        let e = updated(name, "string", None, Some("x")).unwrap_err();
        assert!(e.contains("path or an operator"), "{name}: {e}");
    }
    // Other names, dashes and spaces included, are quoted.
    let s = updated("order-no", "string", None, Some("x")).unwrap();
    assert!(s.commands[0].contains("\"order-no\": \"x\""));
}

#[test]
fn refuses_to_change_the_id_but_accepts_it_unchanged() {
    let columns = names(&["_id", "n"]);
    let types = names(&["objectId", "string"]);
    let mut e = edits(&columns, &types);
    e.updates.push(Update {
        loaded: vec![some(ID), some("1")],
        set: vec![(0, some("65a1b2c3d4e5f60718293a4c"))],
    });
    assert!(
        script(&e)
            .unwrap_err()
            .contains("_id of a document cannot be changed")
    );
    e.updates[0].set = vec![(0, some(ID)), (1, some("2"))];
    let s = script(&e).unwrap();
    assert!(s.commands[0].contains("{ $set: { n: \"2\" } }"));
    // Only the unchanged _id: nothing to run.
    e.updates[0].set = vec![(0, some(ID))];
    assert_eq!(script(&e).unwrap().commands, Vec::<String>::new());
}

#[test]
fn needs_an_id_to_find_a_document() {
    let columns = names(&["n"]);
    let types = names(&["int"]);
    let mut e = edits(&columns, &types);
    e.deletes.push(vec![some("1")]);
    assert!(script(&e).unwrap_err().contains("no _id column"));
    let columns = names(&["_id", "n"]);
    let types = names(&["objectId", "int"]);
    let mut e = edits(&columns, &types);
    e.deletes.push(vec![None, some("1")]);
    assert!(script(&e).unwrap_err().contains("no _id"));
    // An _id column of several types cannot be written.
    let types = names(&["mixed", "int"]);
    let mut e = edits(&columns, &types);
    e.deletes.push(vec![some("1"), some("1")]);
    assert!(script(&e).unwrap_err().contains("column _id"));
}

#[test]
fn ids_of_any_writable_type_find_their_document() {
    for (ty, text, shown) in [
        ("string", "abc", "\"abc\""),
        ("int", "5", "5"),
        ("long", "5", "NumberLong(5)"),
        ("objectId", ID, "ObjectId(\"65a1b2c3d4e5f60718293a4b\")"),
        ("document", "{\"a\":1,\"b\":\"x\"}", "{ a: 1, b: \"x\" }"),
    ] {
        let columns = names(&["_id"]);
        let types = names(&[ty]);
        let mut e = edits(&columns, &types);
        e.deletes.push(vec![some(text)]);
        let s = script(&e).unwrap();
        assert_eq!(
            s.commands,
            [format!("db.orders.deleteOne({{ _id: {shown} }})")]
        );
    }
}

#[test]
fn inserts_without_an_id_make_one_and_undo_by_it() {
    let columns = names(&["_id", "a", "b", "c"]);
    let types = names(&["objectId", "int", "string", "string"]);
    let mut e = edits(&columns, &types);
    e.inserts
        .push(vec![(2, some("x")), (1, some("4")), (3, None)]);
    let s = script(&e).unwrap();
    assert_eq!(s.commands.len(), 1);
    let command = &s.commands[0];
    // A generated ObjectId first, the cells in the order of the columns, NULL ones left out.
    let rest = command
        .strip_prefix("db.orders.insertOne({ _id: ObjectId(\"")
        .unwrap_or_else(|| panic!("{command}"));
    let (hex, rest) = rest.split_once('"').unwrap();
    assert_eq!(hex.len(), 24);
    assert_eq!(rest, "), a: 4, b: \"x\" })");
    assert_eq!(
        s.undo,
        [format!(
            "db.orders.deleteOne({{ _id: ObjectId(\"{hex}\") }})"
        )]
    );
    // Each run makes another id.
    assert_ne!(script(&e).unwrap().commands, s.commands);
}

#[test]
fn inserts_with_an_id_keep_it() {
    let columns = names(&["_id", "a"]);
    let types = names(&["string", "int"]);
    let mut e = edits(&columns, &types);
    e.inserts.push(vec![(0, some("k1")), (1, some("4"))]);
    e.inserts.push(vec![]);
    let s = script(&e).unwrap();
    assert_eq!(s.commands[0], "db.orders.insertOne({ _id: \"k1\", a: 4 })");
    assert_eq!(s.undo[0], "db.orders.deleteOne({ _id: \"k1\" })");
    // A document without any cell is only its id.
    assert!(s.commands[1].starts_with("db.orders.insertOne({ _id: ObjectId("));
    // A NULL _id is no id.
    e.inserts = vec![vec![(0, None), (1, some("4"))]];
    assert!(script(&e).unwrap().commands[0].contains("_id: ObjectId("));
}

#[test]
fn a_new_document_refuses_what_an_edit_refuses() {
    let columns = names(&["_id", "m", "a.b"]);
    let types = names(&["objectId", "mixed", "int"]);
    let mut e = edits(&columns, &types);
    e.inserts.push(vec![(1, some("1"))]);
    assert!(script(&e).unwrap_err().contains("column m"));
    e.inserts = vec![vec![(2, some("1"))]];
    assert!(script(&e).unwrap_err().contains("a.b"));
    // A cell left NULL is not sent: it needs no writable column.
    e.inserts = vec![vec![(1, None), (2, None)]];
    assert!(script(&e).is_ok());
}

#[test]
fn deletes_and_puts_the_document_back() {
    let columns = names(&["_id", "name", "n", "tags", "gone"]);
    let types = names(&["objectId", "string", "int", "array", "string"]);
    let mut e = edits(&columns, &types);
    e.deletes.push(vec![
        some(ID),
        some("Ann"),
        some("3"),
        some("[\"a\",\"b\"]"),
        None,
    ]);
    let s = script(&e).unwrap();
    assert_eq!(
        s.commands,
        [format!(
            "db.orders.deleteOne({{ _id: ObjectId(\"{ID}\") }})"
        )]
    );
    assert_eq!(s.undo.len(), 2);
    assert!(s.undo[0].starts_with("//"));
    assert_eq!(
        s.undo[1],
        format!(
            "db.orders.insertOne({{ _id: ObjectId(\"{ID}\"), name: \"Ann\", n: 3, tags: [\"a\", \"b\"] }})"
        )
    );
}

#[test]
fn a_delete_undo_leaves_out_what_it_cannot_restore_and_says_so() {
    let columns = names(&["_id", "name", "blob", "anything"]);
    let types = names(&["objectId", "string", "binData", "mixed"]);
    let mut e = edits(&columns, &types);
    e.deletes
        .push(vec![some(ID), some("Ann"), some("DEADBEEF"), some("1")]);
    let s = script(&e).unwrap();
    let notes: Vec<_> = s.undo.iter().filter(|l| l.starts_with("//")).collect();
    assert_eq!(notes.len(), 2);
    assert!(notes[1].contains("blob, anything"));
    assert_eq!(
        s.undo.last().unwrap(),
        &format!("db.orders.insertOne({{ _id: ObjectId(\"{ID}\"), name: \"Ann\" }})")
    );
}

#[test]
fn orders_deletes_updates_inserts_and_undoes_in_reverse() {
    let columns = names(&["_id", "n"]);
    let types = names(&["string", "int"]);
    let mut e = edits(&columns, &types);
    e.deletes.push(vec![some("d"), some("1")]);
    e.updates.push(Update {
        loaded: vec![some("u"), some("1")],
        set: vec![(1, some("2"))],
    });
    e.inserts.push(vec![(0, some("i")), (1, some("3"))]);
    let s = script(&e).unwrap();
    let verbs: Vec<_> = s
        .commands
        .iter()
        .map(|c| c.split_once('(').unwrap().0.rsplit('.').next().unwrap())
        .collect();
    assert_eq!(verbs, ["deleteOne", "updateOne", "insertOne"]);
    let undo: Vec<_> = s.undo.iter().filter(|l| !l.starts_with("//")).collect();
    assert!(undo[0].contains("deleteOne") && undo[0].contains("\"i\""));
    assert!(undo[1].contains("updateOne") && undo[1].contains("n: 1"));
    assert!(undo[2].contains("insertOne") && undo[2].contains("\"d\""));
}

#[test]
fn names_the_collection_the_way_the_shell_needs() {
    let columns = names(&["_id"]);
    let types = names(&["int"]);
    for (db, collection, expected) in [
        (None, "orders", "db.orders.deleteOne({ _id: 1 })"),
        (
            None,
            "order items",
            "db.getCollection(\"order items\").deleteOne({ _id: 1 })",
        ),
        (
            None,
            "getter",
            "db.getCollection(\"getter\").deleteOne({ _id: 1 })",
        ),
        (
            Some("my-db"),
            "orders",
            "db.getSiblingDB(\"my-db\").orders.deleteOne({ _id: 1 })",
        ),
    ] {
        let mut e = edits(&columns, &types);
        e.db = db;
        e.collection = collection;
        e.deletes.push(vec![some("1")]);
        let s = script(&e).unwrap();
        assert_eq!(s.commands, [expected]);
        // The shell's parser reads the name back.
        let Ok(Command::Collection {
            db: parsed_db,
            collection: parsed,
            ..
        }) = parse(&s.commands[0])
        else {
            panic!("{}", s.commands[0])
        };
        assert_eq!((parsed_db.as_deref(), parsed.as_str()), (db, collection));
    }
}

#[test]
fn every_statement_and_the_undo_script_parse_in_an_editor_tab() {
    let columns = names(&["_id", "n", "s", "d", "when"]);
    let types = names(&["string", "int", "string", "document", "date"]);
    let mut e = edits(&columns, &types);
    e.deletes.push(vec![
        some("d1"),
        some("1"),
        None,
        some("{\"a\":[1,2]}"),
        some("2024-01-02 03:04:05+00:00"),
    ]);
    e.updates.push(Update {
        loaded: vec![some("u"), some("1"), None, None, None],
        set: vec![
            (1, some("5")),
            (2, some("it's \"quoted\"\nnew line")),
            (3, None),
        ],
    });
    e.inserts
        .push(vec![(0, some("i")), (4, some("2024-01-02"))]);
    let s = script(&e).unwrap();
    for text in [s.commands.join("\n"), s.undo.join("\n")] {
        let statements = split(&text);
        let count = text
            .lines()
            .filter(|l| !l.starts_with("//") && !l.is_empty())
            .count();
        assert_eq!(statements.len(), count, "{text}");
        for range in statements {
            parse(&text[range.clone()]).unwrap_or_else(|e| panic!("{}: {e:?}", &text[range]));
        }
    }
}

#[test]
fn needs_a_type_for_every_column() {
    let columns = names(&["_id", "n"]);
    let types = names(&["int"]);
    assert!(script(&edits(&columns, &types)).is_err());
}

#[test]
fn finds_the_collection_of_a_plain_find() {
    for (src, db, collection) in [
        ("db.orders.find()", None, "orders"),
        (
            "db.orders.find({ n: { $gt: 1 } }).sort({ n: 1 }).skip(5).limit(10)",
            None,
            "orders",
        ),
        ("db.getCollection(\"my coll\").find({})", None, "my coll"),
        (
            "db.getSiblingDB(\"shop\").orders.find();",
            Some("shop"),
            "orders",
        ),
        (
            "  db.orders.find({}, { _id: 1, a: 1, b: true })  ",
            None,
            "orders",
        ),
        ("db.orders.find({}, {})", None, "orders"),
        // Options that leave the documents whole.
        (
            "db.orders.find({}, {}, { comment: 'x', hint: { _id: 1 } }).returnKey(false)",
            None,
            "orders",
        ),
    ] {
        assert_eq!(
            source(src),
            Some(Source {
                db: db.map(str::to_owned),
                collection: collection.to_owned()
            }),
            "{src}"
        );
    }
}

#[test]
fn does_not_edit_what_is_not_a_plain_find() {
    for src in [
        "db.orders.findOne({})",
        "db.orders.aggregate([{ $match: {} }])",
        "db.orders.countDocuments({})",
        "db.orders.distinct(\"a\")",
        "db.orders.deleteMany({})",
        "db.orders.find({}, { a: 0 })",
        "db.orders.find({}, { _id: 0, a: 1 })",
        "db.orders.find({}, { \"a.b\": 1 })",
        "db.orders.find({}, { a: { $slice: 1 } })",
        "db.orders.find({}, { a: \"$b\" })",
        "db.orders.find({}, { a: { $elemMatch: { b: 1 } } })",
        "db.orders.find().returnKey()",
        "db.orders.find({}, {}, { returnKey: true })",
        "db.orders.find().showRecordId()",
        "db.orders.find().explain()",
        "db.orders.find().count()",
        "show dbs",
        "db.orders.find(",
        "SELECT * FROM orders",
        "",
    ] {
        assert_eq!(source(src), None, "{src}");
    }
}
