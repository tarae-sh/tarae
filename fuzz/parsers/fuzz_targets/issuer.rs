#![no_main]
//! An upstream's certificate, its issuer's O and CN read for a refusal's words — `tarae_fuzz_parsers::issuer`.
libfuzzer_sys::fuzz_target!(|b: &[u8]| tarae_fuzz_parsers::issuer(b));
