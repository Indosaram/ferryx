//! Feeds arbitrary bytes, cut at fuzzer-chosen points, into a daemon screen mirror, then checks
//! that its seed survives serde and rebuilds the same seed in a fresh mirror.
#![no_main]

use ferryx_lib::terminal::screen_mirror::{ScreenMirror, ScreenSeed};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&header, rest)) = data.split_first() else {
        return;
    };
    let cols = 2 + u16::from(header & 0x7f);
    let rows = 2 + u16::from(header >> 3);
    let Some(mut mirror) = ScreenMirror::new(cols, rows) else {
        return;
    };
    // 0xff splits the input into separately published chunks.
    for chunk in rest.split(|byte| *byte == 0xff) {
        mirror.feed(chunk);
    }
    let seed = mirror.seed().expect("a fed mirror serializes");
    let json = serde_json::to_string(&seed).expect("seed serializes");
    let decoded: ScreenSeed = serde_json::from_str(&json).expect("seed deserializes");
    assert_eq!(decoded, seed);
    let mut rebuilt = ScreenMirror::from_seed(&decoded).expect("seed rebuilds a mirror");
    let reseed = rebuilt.seed().expect("rebuilt mirror serializes");
    let mut again = ScreenMirror::from_seed(&reseed).expect("reseed rebuilds a mirror");
    assert_eq!(again.seed().expect("third seed"), reseed, "seed is not a fixed point");
});
