use std::collections::HashMap;

use iso_8583_rs::iso8583::new_iso_struct;
use iso_8583_rs::specfile::{FieldDescription, Spec};
use iso_8583_rs::{Builder, CompiledSpec, Error, Message};

const SPEC: &str = "spec1987.yml";

fn spec() -> CompiledSpec {
    CompiledSpec::from_file(SPEC).unwrap()
}

fn sample(spec: &CompiledSpec) -> Vec<u8> {
    let mut b = Builder::new(spec, b"0200").unwrap();
    b.set(2, b"4111111111111111")
        .unwrap()
        .set(3, b"000000")
        .unwrap()
        .set(4, b"000000010000")
        .unwrap()
        .set(11, b"123456")
        .unwrap()
        .set(41, b"TERM0001")
        .unwrap()
        .set(48, b"hello world")
        .unwrap();
    let mut out = Vec::new();
    b.pack_into(&mut out);
    out
}

#[test]
fn build_and_parse_roundtrip() {
    let spec = spec();
    let buf = sample(&spec);
    assert_eq!(
        std::str::from_utf8(&buf).unwrap(),
        "02007020000000810000164111111111111111000000000000010000123456TERM0001011hello world"
    );
    let m = Message::parse(&spec, &buf).unwrap();
    assert_eq!(&m.mti, b"0200");
    assert_eq!(m.get(2), Some(&b"4111111111111111"[..]));
    assert_eq!(m.get_u64(4), Some(10_000));
    assert_eq!(m.get_str(48), Some("hello world"));
    assert_eq!(m.get(7), None);
    let nums: Vec<usize> = m.fields().map(|(n, _)| n).collect();
    assert_eq!(nums, [2, 3, 4, 11, 41, 48]);
}

#[test]
fn secondary_bitmap() {
    let spec = spec();
    let mut b = Builder::new(&spec, b"0200").unwrap();
    b.set(3, b"000000").unwrap().set(102, b"ACC-1").unwrap();
    let mut out = [0u8; 128];
    let n = b.pack_to_slice(&mut out).unwrap();
    let m = Message::parse(&spec, &out[..n]).unwrap();
    assert!(m.bitmap >> 127 == 1, "bit 1 must be set");
    assert_eq!(m.get(102), Some(&b"ACC-1"[..]));
    assert_eq!(m.get(3), Some(&b"000000"[..]));

    // without fields > 64 → no secondary bitmap
    let mut b = Builder::new(&spec, b"0200").unwrap();
    b.set(3, b"000000").unwrap();
    assert_eq!(b.packed_len(), 4 + 16 + 6);
}

#[test]
fn rejects_bad_input_without_panic() {
    let spec = spec();
    let good = sample(&spec);
    // every prefix of a valid message must return Err, not panic
    for i in 0..good.len() {
        assert!(Message::parse(&spec, &good[..i]).is_err(), "prefix {}", i);
    }
    let mut trailing = good.clone();
    trailing.push(b'X');
    assert_eq!(
        Message::parse(&spec, &trailing).unwrap_err(),
        Error::TrailingBytes
    );

    assert_eq!(Message::parse(&spec, b"+200").unwrap_err(), Error::BadMti);
    assert_eq!(
        Message::parse(&spec, b"02007Z00000000000000").unwrap_err(),
        Error::BadHex
    );
    // field 5 is in the bitmap but not in the spec (minimal spec containing only field 3)
    let only_field_3 = CompiledSpec::compile(&Spec {
        fields: HashMap::from([(
            3,
            FieldDescription {
                content_type: "n".into(),
                max_len: 6,
                min_len: 0,
                len_type: "fixed".into(),
                label: String::new(),
            },
        )]),
    })
    .unwrap();
    assert_eq!(
        Message::parse(&only_field_3, b"02000800000000000000").unwrap_err(),
        Error::UnknownField(5)
    );
    // bit 65 (tertiary) is rejected
    assert_eq!(
        Message::parse(&spec, b"020080000000000000008000000000000000").unwrap_err(),
        Error::TertiaryUnsupported
    );
    // non-ASCII does not panic
    assert!(Message::parse(&spec, "02é0".as_bytes()).is_err());

    // strict rejects invalid content, lazy accepts and then rejects upon read
    let mut bad = good.clone();
    let pos = bad.iter().position(|&c| c == b'4').unwrap(); // first PAN digit
    bad[pos] = b'X';
    assert_eq!(Message::parse(&spec, &bad).unwrap_err(), Error::Invalid(2));
    let lazy = Message::parse_lazy(&spec, &bad).unwrap();
    assert_eq!(lazy.get_validated(&spec, 2).unwrap_err(), Error::Invalid(2));
    assert!(lazy.get_validated(&spec, 3).unwrap().is_some());
}

#[test]
fn builder_validates() {
    let spec = spec();
    let mut b = Builder::new(&spec, b"0200").unwrap();
    assert_eq!(b.set(3, b"12345").unwrap_err(), Error::BadLength(3)); // fixed 6
    assert_eq!(b.set(3, b"12345a").unwrap_err(), Error::Invalid(3));
    assert_eq!(b.set(65, b"x").unwrap_err(), Error::UnknownField(65));
    assert_eq!(b.set(1, b"x").unwrap_err(), Error::UnknownField(1));
    assert_eq!(b.set(2, b"41").unwrap_err(), Error::BadLength(2)); // PAN MinLen 12
    assert!(Builder::new(&spec, b"02a0").is_err());
    // overwriting maintains packed_len
    b.set(48, b"abc").unwrap().set(48, b"a").unwrap();
    assert_eq!(b.packed_len(), 4 + 16 + 3 + 1);
    let mut small = [0u8; 10];
    assert_eq!(b.pack_to_slice(&mut small).unwrap_err(), Error::TooShort);
}

#[test]
fn legacy_api_roundtrip() {
    let mut iso = new_iso_struct(SPEC, false).unwrap();
    iso.add_mti("0200").unwrap();
    iso.add_field(2, "4111111111111111").unwrap();
    iso.add_field(3, "000000").unwrap();
    iso.add_field(48, "hello").unwrap();
    assert!(iso.add_field(65, "x").is_err());
    let s = iso.to_string().unwrap();
    let parsed = iso.parse(&s).unwrap();
    assert_eq!(parsed.mti.mti, "0200");
    assert_eq!(parsed.bitmap, iso.bitmap);
    assert_eq!(
        parsed.elements.get_elements().get(&48).map(String::as_str),
        Some("hello")
    );

    // short / malformed input → Err, not panic
    assert!(iso.parse("02").is_err());
    assert!(iso.parse("0200zz").is_err());
    // field in bitmap without a value → Err, not panic
    iso.bitmap[6] = 1;
    assert!(iso.to_string().is_err());
}

#[test]
fn get_matches_set_for_mixed_len_types() {
    let spec = spec();
    // var → var → fixed → var, including empty var field (min_len 0)
    let vals: [(usize, &[u8]); 6] = [
        (2, b"4111111111111111"),
        (3, b"000000"),
        (48, b""),
        (49, b"360"),
        (102, b"ACC"),
        (4, b"000000000001"),
    ];
    let mut b = Builder::new(&spec, b"0210").unwrap();
    for (n, v) in vals {
        b.set(n, v).unwrap();
    }
    let mut out = Vec::new();
    b.pack_into(&mut out);
    let m = Message::parse(&spec, &out).unwrap();
    for (n, v) in vals {
        assert_eq!(m.get(n), Some(v), "field {}", n);
    }
    let mut sorted = vals;
    sorted.sort();
    assert!(m.fields().eq(sorted.iter().map(|&(n, v)| (n, v))));
}
