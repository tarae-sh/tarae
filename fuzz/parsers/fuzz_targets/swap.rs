#![no_main]
//! A body's payload through masking's scan, planted and cut — `tarae_fuzz_parsers::swap`.
libfuzzer_sys::fuzz_target!(|b: &[u8]| tarae_fuzz_parsers::swap(b));
