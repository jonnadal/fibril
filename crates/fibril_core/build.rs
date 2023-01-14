use rustc_version::{version_meta, Channel};

// Based on:
// https://stackoverflow.com/questions/61417452/how-to-get-a-feature-requirement-tag-in-the-documentation-generated-by-cargo-do/70914430#70914430
fn main() {
    if version_meta().unwrap().channel == Channel::Nightly {
        println!("cargo:rustc-cfg=CHANNEL_NIGHTLY");
    }
}
