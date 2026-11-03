#![no_main]
//! A host's or an interface's stream, as the other side reads it — `tarae_fuzz_parsers::wire_decode`.
libfuzzer_sys::fuzz_target!(|b: &[u8]| tarae_fuzz_parsers::wire_decode(b));
