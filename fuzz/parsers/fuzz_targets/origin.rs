#![no_main]
//! A request's head inside a tunnel tarae terminates, its body after it — `tarae_fuzz_parsers::origin`.
libfuzzer_sys::fuzz_target!(|b: &[u8]| tarae_fuzz_parsers::origin(b));
