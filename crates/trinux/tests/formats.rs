//! The ternary data formats (docs/FORMATS.md) in Trinux: the libraries'
//! own checks (fmttest), and the tools on files from the binary border
//! (WAV, PPM, PGM made here), with their round trips, the picture on the
//! framebuffer, and their errors.

mod common;
#[allow(unused_imports)]
use common::{has as assert_has};

use std::path::PathBuf;

/// Boot `disk`, log in as root and type each line at the prompt.
fn session(disk: &[i16], lines: &[&str], max_steps: u64) -> (String, setun_core::Machine) {
    let mut m = trinux::boot_trytes(disk, 729, 0, "");
    let mut out = String::new();
    let (mut next, mut steps) = (0, 0);
    let mut logged_in = false;
    while steps < max_steps && !m.powered_off {
        m.run(1_000_000);
        steps += 1_000_000;
        out.push_str(&m.dev.serial.take_output());
        if out.contains("Kernel panic") {
            break;
        }
        if !logged_in && out.ends_with("login: ") {
            m.dev.serial_input("root\n");
            continue;
        }
        if !logged_in && out.ends_with("Password: ") {
            m.dev.serial_input("root\n");
            logged_in = true;
            continue;
        }
        let Some(line) = lines.get(next) else {
            if out.ends_with("$ ") {
                break;
            }
            continue;
        };
        if out.ends_with("$ ") {
            m.dev.serial_input(&format!("{line}\n"));
            next += 1;
        }
    }
    (out, m)
}


/// A PCM WAV with the 44-byte header tau2wav writes.
fn wav(channels: u16, rate: u32, bits: u16, samples: &[i32]) -> Vec<u8> {
    let bps = (bits / 8) as u32;
    let data = samples.len() as u32 * bps;
    let mut v = b"RIFF".to_vec();
    v.extend((36 + data).to_le_bytes());
    v.extend(b"WAVEfmt ");
    v.extend(16u32.to_le_bytes());
    v.extend(1u16.to_le_bytes());
    v.extend(channels.to_le_bytes());
    v.extend(rate.to_le_bytes());
    v.extend((rate * channels as u32 * bps).to_le_bytes());
    v.extend((channels as u32 * bps).to_le_bytes()[..2].to_vec());
    v.extend(bits.to_le_bytes());
    v.extend(b"data");
    v.extend(data.to_le_bytes());
    for &s in samples {
        let s = if bits == 8 { s + 128 } else { s };
        v.extend(&s.to_le_bytes()[..bps as usize]);
    }
    v
}

fn pnm(magic: &str, w: usize, h: usize, ch: usize) -> Vec<u8> {
    let mut v = format!("{magic}\n{w} {h}\n255\n").into_bytes();
    for k in 0..w * h * ch {
        v.push(if k % 37 == 0 { 255 } else if k < w * ch * 3 { 128 } else { ((k * 7 + k / 13) % 256) as u8 });
    }
    v
}

fn disk(extra: &[(&str, Vec<u8>)]) -> Vec<i16> {
    let dir = std::env::temp_dir().join(format!("setun-formats-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut files = trinux::default_files();
    for (name, bytes) in extra {
        let p: PathBuf = dir.join(name.rsplit('/').next().unwrap());
        std::fs::write(&p, bytes).unwrap();
        files.push((name.to_string(), p));
    }
    trinux::build_disk_with(&trinux::kernel_dir(), &files).unwrap_or_else(|e| panic!("{e}")).trytes
}

#[test]
fn the_libraries_check_themselves() {
    let (out, _) = session(&disk(&[]), &["fmttest"], 3_000_000_000);
    assert_has(&out, &["fmttest: all passed"]);
    assert!(!out.contains("FAILED"), "{out}");
}

/// The published vectors (docs/FORMATS.md §5) against the libraries, and
/// the checker against a vector made wrong on purpose.
#[test]
fn the_libraries_pass_the_conformance_vectors() {
    let vectors = std::fs::read_to_string(trinux::user_dir().join("share/fmtvectors.txt")).unwrap();
    let count = vectors.lines().filter(|l| !l.starts_with('#') && !l.is_empty()).count();
    assert!(count > 200, "{count}");
    let broken = "tvar 7 = 8\ntvar! 7\nttv true = 1 0\nttv! 1 1\ntim! 0\n";
    let d = disk(&[("/home/broken.txt", broken.as_bytes().to_vec())]);
    let (out, _) = session(&d, &["fmtvec", "fmtvec /home/broken.txt; echo status $?", "fmtvec /nope; echo status $?"], 6_000_000_000);
    assert_has(
        &out,
        &[
            &format!("fmtvec: {count} vectors, all passed"),
            "fmtvec: line 1: tvar_put_long gives other trytes\nfmtvec: line 1: tvar_get_long reads another value\nfmtvec: line 1: tvar_put gives other trytes\nfmtvec: line 1: tvar_get reads another value\n",
            "fmtvec: line 2: tvar_get accepts it\nfmtvec: line 2: tvar_get_long accepts it\n",
            "fmtvec: line 3: ttv_parse encodes it differently\nfmtvec: line 3: ttv_format prints other text\n",
            "fmtvec: line 4: ttv_format accepts it\n",
            "fmtvec: 5 vectors, ",
            " failed\nstatus 1",
            "fmtvec: /nope: No such file or directory\nstatus 2",
        ],
    );
}

#[test]
fn sound_from_and_to_wav() {
    let s16: Vec<i32> = (0..100).map(|k| if k == 0 { -32768 } else if k == 1 { 32767 } else { (k * 977 % 65536) - 32768 }).collect();
    let s8: Vec<i32> = (0..60).map(|k| (k * 13 % 256) - 128).collect();
    let s24: Vec<i32> = (0..40).map(|k| (k * 419_431 % 16_777_216) - 8_388_608).collect();
    let d = disk(&[
        ("/home/a16.wav", wav(2, 8000, 16, &s16)),
        ("/home/a8.wav", wav(1, 11025, 8, &s8)),
        ("/home/a24.wav", wav(1, 48000, 24, &s24)),
        ("/home/text.wav", b"not a wave".to_vec()),
    ]);
    let (out, _) = session(
        &d,
        &[
            "wav2tau /home/a16.wav /tmp/a16.tau; tauinfo /tmp/a16.tau",
            "tau2wav /tmp/a16.tau /tmp/b16.wav; md5sum /home/a16.wav /tmp/b16.wav",
            "wav2tau /home/a8.wav /tmp/a8.tau; tauinfo /tmp/a8.tau; tau2wav -b 8 /tmp/a8.tau /tmp/b8.wav; md5sum /home/a8.wav /tmp/b8.wav",
            "wav2tau /home/a24.wav /tmp/a24.tau; tauinfo /tmp/a24.tau; tau2wav -b 24 /tmp/a24.tau /tmp/b24.wav; md5sum /home/a24.wav /tmp/b24.wav",
            "tone -r 8000 440 0.01 /tmp/t.tau; tauinfo /tmp/t.tau",
            "tone -a 2 440 1 /tmp/x.tau; echo status $?",
            "wav2tau /home/text.wav /tmp/x.tau; echo status $?",
            "tauinfo /home/a16.wav; echo status $?",
            "tau2wav -b 12 /tmp/a16.tau /tmp/x.wav; echo status $?",
        ],
        3_000_000_000,
    );
    let md5 = |name: &str| -> String {
        out.lines().find(|l| l.ends_with(&format!("  {name}")) && !l.starts_with('$')).and_then(|l| l.split_whitespace().next()).unwrap_or("").to_string()
    };
    for (a, b) in [("/home/a16.wav", "/tmp/b16.wav"), ("/home/a8.wav", "/tmp/b8.wav"), ("/home/a24.wav", "/tmp/b24.wav")] {
        assert!(!md5(a).is_empty() && md5(a) == md5(b), "{a} and {b} differ:\n{out}");
    }
    assert_has(
        &out,
        &[
            "/tmp/a16.tau: TAU, 8000 Hz, 2 channels, 50 frames (0.00625 s), peak 0.3",
            "/tmp/a8.tau: TAU, 11025 Hz, 1 channel, 60 frames (0.00544218 s), peak 0.3512 of full scale",
            "/tmp/a24.tau: TAU, 48000 Hz, 1 channel, 40 frames (0.000833333 s), peak 0.1299 of full scale",
            "/tmp/t.tau: TAU, 8000 Hz, 1 channel, 80 frames (0.01 s), peak 0.5",
            "usage: tone [-r rate] [-a amplitude] freq seconds out.tau\nstatus 2",
            "wav2tau: not a WAV file: /home/text.wav\nstatus 1",
            "tauinfo: not a TAU file: /home/a16.wav\nstatus 1",
            "usage: tau2wav [-b 8|16|24] in.tau out.wav\nstatus 2",
        ],
    );
}

#[test]
fn pictures_from_and_to_ppm() {
    let ppm = pnm("P6", 20, 10, 3);
    let d = disk(&[("/home/p.ppm", ppm.clone()), ("/home/g.pgm", pnm("P5", 9, 9, 1)), ("/home/bad.ppm", b"P3\n1 1\n255\n0 0 0\n".to_vec())]);
    let (out, m) = session(
        &d,
        &[
            "ppm2tim /home/p.ppm /tmp/p.tim; timinfo /tmp/p.tim; tim2ppm /tmp/p.tim /tmp/q.ppm; md5sum /home/p.ppm /tmp/q.ppm",
            "ppm2tim -r /home/p.ppm /tmp/r.tim; timinfo /tmp/r.tim; tim2ppm /tmp/r.tim /tmp/r.ppm; md5sum /tmp/r.ppm",
            "ppm2tim /home/g.pgm /tmp/g.tim; timinfo /tmp/g.tim; tim2ppm /tmp/g.tim /tmp/g.pgm; md5sum /home/g.pgm /tmp/g.pgm",
            "ppm2tim /home/bad.ppm /tmp/x.tim; echo status $?",
            "timinfo /home/p.ppm; echo status $?",
            "ttv -c '{\"pic\": [1, true, unknown, false, -5000000000000, 2.5, x\"00ff\", null, \"Сетунь\"]}' /tmp/v.ttv; ttv /tmp/v.ttv",
            "ttv -c '[1, 2' /tmp/w.ttv; echo status $?",
            "ttv /home/p.ppm; echo status $?",
        ],
        3_000_000_000,
    );
    let md5 = |name: &str| -> String {
        out.lines().find(|l| l.ends_with(&format!("  {name}")) && !l.starts_with('$')).and_then(|l| l.split_whitespace().next()).unwrap_or("").to_string()
    };
    assert!(!md5("/home/p.ppm").is_empty() && md5("/home/p.ppm") == md5("/tmp/q.ppm"), "{out}");
    assert_eq!(md5("/home/p.ppm"), md5("/tmp/r.ppm"), "{out}");
    assert_eq!(md5("/home/g.pgm"), md5("/tmp/g.pgm"), "{out}");
    assert_has(
        &out,
        &[
            "/tmp/p.tim: TIM 20x10 RGB, 6 trits a sample, ",
            "/tmp/r.tim: TIM 20x10 RGB, 6 trits a sample, 624 trytes (raw 624, 0% saved)",
            "/tmp/g.tim: TIM 9x9 grey, 6 trits a sample, ",
            "ppm2tim: not a binary PPM (P6) or PGM (P5): /home/bad.ppm\nstatus 1",
            "timinfo: not a valid TIM file: /home/p.ppm\nstatus 1",
            "{\"pic\": [1, true, unknown, false, -5000000000000, 2.5, x\"00ff\", null, \"Сетунь\"]}",
            "ttv: cannot read the notation at: \nstatus 1",
            "ttv: not valid TTV: /home/p.ppm\nstatus 1",
        ],
    );
    // timview's picture is on the screen while it waits for a key, centred,
    // each sample rounded to the display's 3 trits, on mid-grey
    let mut m = m;
    m.dev.serial_input("timview /tmp/p.tim; echo status $?\n");
    let (x0, y0) = ((640 - 20) / 2, (480 - 10) / 2);
    let tri = |v: u8| -> i64 { ((v as i64 - 128) as f64 / 27.0).round() as i64 };
    let data = &ppm[ppm.len() - 600..];
    let want = |x: usize, y: usize| {
        let p = &data[(y * 20 + x) * 3..];
        tri(p[0]) * 729 + tri(p[1]) * 27 + tri(p[2])
    };
    let mut steps = 0;
    while steps < 200_000_000 && m.mem.framebuffer()[(y0 + 9) * 640 + x0 + 19] as i64 != want(19, 9) {
        m.run(1_000_000);
        steps += 1_000_000;
    }
    let fb = m.mem.framebuffer();
    for y in 0..10 {
        for x in 0..20 {
            assert_eq!(fb[(y0 + y) * 640 + x0 + x] as i64, want(x, y), "pixel ({x}, {y})");
        }
    }
    assert!(fb[..y0 * 640].iter().all(|&p| p == 0), "the rest of the screen is mid-grey");
    // Enter: the text console comes back, fresh, with what follows on it
    m.dev.serial_input("\n");
    let mut out = String::new();
    while steps < 400_000_000 && !out.ends_with("$ ") {
        m.run(1_000_000);
        steps += 1_000_000;
        out.push_str(&m.dev.serial.take_output());
    }
    assert!(out.contains("status 0"), "{out}");
    let fb = m.mem.framebuffer();
    let row: String = (0..8).map(|c| char::from_u32(fb[3 * c] as u32).unwrap_or('?')).collect();
    assert_eq!(row, "status 0", "the text screen is back");
}
