//! Modular system documents: a rack carries a rill-lang program plus its
//! control modules, and round-trips through JSON.

#[cfg(feature = "serialization")]
use rill_adrift::modular::serialization::{from_json, to_json, ModularSystemDef, RackDef};

#[cfg(feature = "serialization")]
#[test]
fn rack_program_roundtrips_through_json() {
    let program = "main = do { write (Just \"hi\") stdout; return () }";
    let rack = RackDef::new("main", program);
    let def = ModularSystemDef {
        format_version: "1.0".to_string(),
        sample_rate: 48_000.0,
        block_size: 256,
        racks: vec![rack],
        description: None,
    };

    let json = to_json(&def).expect("serialize");
    let back = from_json(&json).expect("deserialize");

    assert_eq!(back.racks.len(), 1);
    assert_eq!(back.racks[0].name, "main");
    assert_eq!(back.racks[0].program, program);
    assert!(back.racks[0].args.is_empty());
    assert_eq!(back.sample_rate, 48_000.0);
}

#[cfg(feature = "serialization")]
#[test]
fn rack_args_are_part_of_the_document() {
    let mut rack = RackDef::new("greeter", "main name = write (Just name) stdout");
    rack.args.push("world".to_string());
    let def = ModularSystemDef {
        format_version: "1.0".to_string(),
        sample_rate: 44_100.0,
        block_size: 256,
        racks: vec![rack],
        description: Some("one greeter".to_string()),
    };

    let json = to_json(&def).expect("serialize");
    let back = from_json(&json).expect("deserialize");

    assert_eq!(back.racks[0].args, ["world"]);
    assert_eq!(back.description.as_deref(), Some("one greeter"));
}
