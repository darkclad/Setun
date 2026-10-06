//! The ternary idioms of ISA v0.7 in the assembler and disassembler:
//! MRG, TCP, TCN, TREV in every form, and STWM.

use setun_asm::{disasm, text};

fn words(src: &str) -> Vec<ternary::Word> {
    let p = text::assemble(&format!(".org 0\n{src}\n")).unwrap_or_else(|e| panic!("{src}: {e}"));
    p.segments.iter().flat_map(|s| s.words.clone()).collect()
}

#[test]
fn idioms_round_trip() {
    for src in [
        "mrg r1, r2, r3",
        "mrg r1, r2, -13",
        "mrg r1, r2, 5555555",
        "tcp r4, r5, 0",
        "tcn r4, r5, 9",
        "trev r6, r7, r8",
        "trev r6, r7, 27",
        "stwm r1, [r2], r3",
        "stwm r-13, [r13], r0",
        "cas3 r1, [r2], r3",
    ] {
        let w = words(src);
        let (text, n) = disasm::disasm_one(w[0], w.get(1).copied(), 0);
        assert_eq!(n, w.len(), "{src}: length");
        assert_eq!(words(&text), w, "{src} -> {text}: not the same words");
        let name = src.split(' ').next().unwrap();
        assert!(text.starts_with(name), "{src} -> {text}");
    }
}

#[test]
fn idioms_refused() {
    let e = |src: &str| text::assemble(&format!(".org 0\n{src}\n")).unwrap_err().msg;
    assert!(e("stwm r1, [r2+3], r3").contains("no offset"), "{}", e("stwm r1, [r2+3], r3"));
    assert!(e("stwm r1, [r2], 5").contains("register expected"), "{}", e("stwm r1, [r2], 5"));
    assert!(e("stwm r1, r2, r3").contains("bad operands"), "{}", e("stwm r1, r2, r3"));
    assert!(e("mrg r1, r2").contains("bad operands"), "{}", e("mrg r1, r2"));
    assert!(e("cas3 r1, [r2-3], r3").contains("no offset"), "{}", e("cas3 r1, [r2-3], r3"));
    assert!(e("cas3 r1, r2, r3").contains("bad operands"), "{}", e("cas3 r1, r2, r3"));
}
