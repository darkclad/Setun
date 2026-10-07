//! The vector unit (ISA v0.7 §15) in the assembler and disassembler.

use setun_asm::{disasm, text};
use setun_core::isa::{self, alu, op, valu, vmem, vmov};

fn words(src: &str) -> Vec<ternary::Word> {
    let p = text::assemble(&format!(".org 0\n{src}\n")).unwrap_or_else(|e| panic!("{src}: {e}"));
    p.segments.iter().flat_map(|s| s.words.clone()).collect()
}

#[test]
fn vector_round_trip() {
    let mut srcs: Vec<String> = Vec::new();
    for name in ["add", "sub", "min", "max", "tmul", "tsat", "cons"] {
        srcs.push(format!("v{name} v1, v2, v3"));
        srcs.push(format!("v{name}w v-13, v13, v0"));
    }
    for name in ["neg", "abs", "sgn"] {
        srcs.push(format!("v{name} v4, v5"));
        srcs.push(format!("v{name}w v4, v5"));
    }
    for src in [
        "vmrg v1, v2, v3",
        "vsum r1, v2",
        "vdotw r-5, v6, v7",
        "vld v1, [r2]",
        "vld v1, [r2+117]",
        "vst v-1, [r13-117]",
        "vst v2, [r3+9]",
        "vget r1, v2, 8",
        "vput v1, r2, 0",
        "vgetw r1, v2, 2",
        "vputw v1, r2, 1",
        "vbcst v1, r2",
        "vbcstw v13, r-13",
    ] {
        srcs.push(src.into());
    }
    for src in &srcs {
        let w = words(src);
        assert_eq!(w.len(), 1, "{src}: one word");
        let (text, n) = disasm::disasm_one(w[0], None, 0);
        assert_eq!(n, 1, "{src}: length");
        assert_eq!(words(&text), w, "{src} -> {text}: not the same words");
        let name = src.split(' ').next().unwrap();
        assert!(text.starts_with(&format!("{name} ")), "{src} -> {text}");
    }
}

#[test]
fn vector_encodings() {
    // the fields the CPU reads (crates/core: exec_vec)
    assert_eq!(words("vadd v1, v2, v3"), [isa::rrr(op::VALU, alu::ADD, 1, 2, 3)]);
    assert_eq!(words("vtmulw v1, v2, v3"), [isa::rrr(op::VALU, alu::TMUL + valu::WORD_LANES, 1, 2, 3)]);
    assert_eq!(words("vneg v1, v2"), [isa::rrr(op::VALU, alu::NEG, 1, 2, 0)]);
    assert_eq!(words("vdotw r4, v2, v3"), [isa::rrr(op::VALU, valu::VDOTW, 4, 2, 3)]);
    assert_eq!(words("vst v1, [r2-18]"), [isa::rrr(op::VMEM, vmem::VST, 1, 2, -2)]);
    assert_eq!(words("vput v5, r10, 8"), [isa::rrr(op::VMOV, vmov::VPUT, 5, 10, 8)]);
    assert_eq!(words("vgetw r8, v1, 2"), [isa::rrr(op::VMOV, vmov::VGETW, 8, 1, 2)]);
}

#[test]
fn vector_refused() {
    let e = |src: &str| text::assemble(&format!(".org 0\n{src}\n")).unwrap_err().msg;
    let has = |src: &str, what: &str| {
        let m = e(src);
        assert!(m.contains(what), "{src}: {m}");
    };
    has("vld v1, [r2+3]", "not a multiple of 9");
    has("vst v1, [r2+126]", "out of range");
    has("vld r1, [r2]", "vector register expected");
    has("vadd v1, v2, r3", "vector register expected");
    has("vadd v1, v2", "bad operands");
    has("vneg v1, v2, v3", "bad operands");
    has("vsum v1, v2", "register expected");
    has("vget r1, v2, 9", "lane 9 out of range");
    has("vputw v1, r2, 3", "lane 3 out of range");
    has("vget r1, v2, -1", "out of range");
    has("vbcst v1, v2", "register expected");
    has("vmul v1, v2, v3", "unknown instruction"); // not lane-wise
    has("vdivw v1, v2, v3", "unknown instruction");
    has("vadd v14, v1, v2", "vector register expected");
    has("vld v1, r2", "bad operands");
    // the disassembler refuses what the CPU refuses
    for w in [
        isa::rrr(op::VALU, alu::MUL, 1, 2, 3),
        isa::rrr(op::VALU, 58, 1, 2, 3),
        isa::rrr(op::VMEM, 3, 1, 2, 0),
        isa::rrr(op::VMOV, 7, 1, 2, 0),
    ] {
        let (text, _) = disasm::disasm_one(w, None, 0);
        assert!(!text.starts_with('v'), "{text}");
    }
}
