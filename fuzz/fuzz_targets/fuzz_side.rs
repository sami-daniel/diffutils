#![no_main]
#[macro_use]
extern crate libfuzzer_sys;

use diffutilslib::side_diff;

use std::fs::File;
use std::io::Write;

fuzz_target!(|x: (Vec<u8>, Vec<u8>)| {
    let (original, new) = x;
    
    let mut output_buf = vec![];
    side_diff::diff(&original, &new, &mut output_buf);
    File::create("target/fuzz.file.original")
        .unwrap()
        .write_all(&original)
        .unwrap();
    File::create("target/fuzz.file.new")
        .unwrap()
        .write_all(&new)
        .unwrap();
    File::create("target/fuzz.file")
        .unwrap()
        .write_all(&original)
        .unwrap();
    File::create("target/fuzz.diff")
        .unwrap()
        .write_all(&output_buf)
        .unwrap();
});