//! /usr/share/unifont: every glyph reads back from the packed file as
//! GNU Unifont has it, and what is not there reads as nothing.

use trinux::unifont;

#[test]
fn every_glyph_reads_back() {
    let glyphs = unifont::load(&unifont::sources()).unwrap();
    let file = trinux::unifont_file().unwrap();
    assert_eq!(&file[..3], &['U' as i16, 'F' as i16, 1]);
    assert!(glyphs.len() > 100_000, "planes 0 and 1: {}", glyphs.len());
    for (&cp, &want) in &glyphs {
        assert_eq!(unifont::glyph(&file, cp), Some(want), "U+{cp:04X}");
    }
    // the shapes we rely on: 'A' is 8 wide, 日 and 😀 16, all with ink
    for (cp, w) in [('A' as u32, 8), ('日' as u32, 16), (0x1F600, 16), ('Ж' as u32, 8)] {
        let (width, rows) = glyphs[&cp];
        assert_eq!(width, w, "U+{cp:04X}");
        assert!(rows.iter().any(|&r| r != 0), "U+{cp:04X} is blank");
    }
    // nothing: code points the font has no glyph for (it even draws boxes
    // for many unassigned ones), the surrogates, plane 2
    let absent: Vec<u32> = (0..0x20000).filter(|cp| !glyphs.contains_key(cp)).collect();
    assert!(absent.len() > 1000, "{}", absent.len());
    for &cp in absent.iter().step_by(97) {
        assert_eq!(unifont::glyph(&file, cp), None, "U+{cp:04X}");
    }
    assert_eq!(unifont::glyph(&file, 0xD800), None);
    assert_eq!(unifont::glyph(&file, 0x20000), None, "plane 2 is left out");
    // every stored tryte is a tryte
    assert!(file.iter().all(|&t| (-9841..=9841).contains(&t)));
}

#[test]
fn it_fits_on_the_disk() {
    let file = trinux::unifont_file().unwrap();
    let blocks = file.len().div_ceil(19_683);
    assert!(blocks <= 110, "{blocks} blocks");
}
