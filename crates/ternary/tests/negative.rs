//! Negative tests: invalid input, out-of-range values and misuse.

use ternary::{ParseError, Tern, Tri, Tryte, Word};

#[test]
fn construction_out_of_range() {
    assert!(Tryte::new(9842).is_none());
    assert!(Tryte::new(-9842).is_none());
    assert!(Tri::new(14).is_none());
    assert!(Tri::new(-14).is_none());
    assert!(Word::new(Word::MAX.value() + 1).is_none());
    assert!(Word::new(Word::MIN.value() - 1).is_none());
    assert!(Word::new(i64::MAX).is_none());
    assert!(Tern::<1>::new(2).is_none());
}

#[test]
fn checked_add_detects_overflow() {
    assert_eq!(Tryte::MAX.checked_add(Tryte::new(1).unwrap()), None);
    assert_eq!(Tryte::MIN.checked_add(Tryte::new(-1).unwrap()), None);
    assert_eq!(Tryte::MIN.wrapping_sub(Tryte::new(1).unwrap()), Tryte::MAX);
}

const O: char = ternary::OVERLINE;

#[test]
fn trit_text_rejects_garbage() {
    assert_eq!("".parse::<Tryte>(), Err(ParseError::Empty));
    assert_eq!("  _ ".parse::<Tryte>(), Err(ParseError::Empty));
    assert_eq!("10x".parse::<Tryte>(), Err(ParseError::BadChar('x')));
    assert_eq!("2".parse::<Tryte>(), Err(ParseError::BadChar('2')));
    assert_eq!("1T01T01T01".parse::<Tryte>(), Err(ParseError::Overflow), "10 trits");
    assert_eq!("1111".parse::<Tri>(), Err(ParseError::Overflow));
    // the old + and - digits are gone
    assert_eq!("+0-".parse::<Tryte>(), Err(ParseError::BadChar('+')));
    assert_eq!("10-".parse::<Tryte>(), Err(ParseError::BadChar('-')));
    // overlines: only on 1, once, after a digit
    assert_eq!(format!("10{O}").parse::<Tryte>(), Err(ParseError::BadOverline('0')), "no -0");
    assert_eq!(format!("1T{O}").parse::<Tryte>(), Err(ParseError::BadOverline('T')), "T is already negative");
    assert_eq!(format!("11{O}{O}").parse::<Tryte>(), Err(ParseError::DoubleOverline('1')));
    assert_eq!(format!("{O}1").parse::<Tryte>(), Err(ParseError::StrayOverline));
    assert_eq!(format!("1_{O}0").parse::<Tryte>(), Err(ParseError::BadOverline('_')));
    assert_eq!(format!("2{O}").parse::<Tryte>(), Err(ParseError::BadOverline('2')), "base 3 has no 2");
}

#[test]
fn base27_rejects_garbage() {
    assert_eq!(Tryte::from_b27(""), Err(ParseError::Empty));
    // E..Z are not digits (only 0-9 and A-D are), and neither are the old letters
    for bad in ['E', 'e', 'M', 'm', 'N', 'Z', 'z', 'T', '-', '+', 'Ж'] {
        assert_eq!(Tryte::from_b27(&format!("0{bad}0")), Err(ParseError::BadChar(bad)), "{bad}");
    }
    assert_eq!(Tryte::from_b27("1000"), Err(ParseError::Overflow), "4 digits > 9 trits");
    assert_eq!(Tri::from_b27("10"), Err(ParseError::Overflow));
    // a long run of digits must not overflow the parser itself
    assert_eq!(Word::from_b27(&"D".repeat(60)), Err(ParseError::Overflow));
    // leading zeros are fine
    assert_eq!(Tryte::from_b27("00000001").unwrap().value(), 1);
    // overlines: on a nonzero digit, once, after it
    assert_eq!(Tryte::from_b27(&format!("E{O}")), Err(ParseError::BadOverline('E')));
    assert_eq!(Tryte::from_b27(&format!("_{O}1")), Err(ParseError::BadOverline('_')));
    assert_eq!(Tryte::from_b27(&format!("10{O}")), Err(ParseError::BadOverline('0')), "no -0");
    assert_eq!(Tryte::from_b27(&format!("1D{O}{O}")), Err(ParseError::DoubleOverline('D')));
    assert_eq!(Tryte::from_b27(&format!("{O}D")), Err(ParseError::StrayOverline));
    assert_eq!(Tryte::from_b27(&format!("1 {O}")), Err(ParseError::BadOverline(' ')));
}

#[test]
fn parse_errors_display() {
    assert_eq!(ParseError::Empty.to_string(), "empty ternary literal");
    assert!(ParseError::BadChar('q').to_string().contains("'q'"));
    assert_eq!(ParseError::Overflow.to_string(), "ternary literal does not fit");
    assert_eq!(ParseError::BadOverline('0').to_string(), "'0' cannot be overlined");
    assert_eq!(ParseError::DoubleOverline('D').to_string(), "'D' has two overlines");
    assert_eq!(ParseError::StrayOverline.to_string(), "an overline with no digit before it");
}

#[test]
#[should_panic(expected = "trit index 0")]
fn trit_index_zero_panics() {
    Tryte::ZERO.trit(0);
}

#[test]
#[should_panic(expected = "trit index 10")]
fn trit_index_past_width_panics() {
    Tryte::ZERO.trit(10);
}

#[test]
#[should_panic(expected = "field 0:3")]
fn field_starting_at_zero_panics() {
    Tryte::ZERO.field(0..=3);
}

#[test]
#[should_panic(expected = "field 5:3")]
fn reversed_field_panics() {
    #[allow(clippy::reversed_empty_ranges)]
    Tryte::ZERO.field(5..=3);
}

#[test]
#[should_panic(expected = "field 8:10")]
fn field_past_width_panics() {
    Tryte::ZERO.with_field(8..=10, 1);
}

#[test]
fn with_field_wraps_oversized_values() {
    // Only the low trits of the value land in the field.
    let t = Tryte::ZERO.with_field(8..=9, 5); // 5 = +-- → low 2 trits "TT" = −4
    assert_eq!(t.field(8..=9), -4);
    assert_eq!(t.field(1..=7), 0, "nothing leaks outside the field");
}

#[test]
fn shifts_past_the_width_clear_everything() {
    assert_eq!(Tryte::MAX.shift(9), Tryte::ZERO);
    assert_eq!(Tryte::MAX.shift(40), Tryte::ZERO);
    assert_eq!(Tryte::MAX.shift(-10), Tryte::ZERO);
    assert_eq!(Tryte::MIN.shift(-40), Tryte::ZERO);
    // Just inside the width, one trit survives each way.
    assert_eq!(Tryte::MAX.shift(-8).value(), 1);
    assert_eq!(Tryte::new(1).unwrap().shift(8).value(), 6561);
}

#[test]
fn resize_narrowing_drops_high_trits() {
    let w = Word::new(9842).unwrap(); // needs 10 trits
    assert_eq!(w.resize::<9>().value(), 9842 - 19_683);
}
