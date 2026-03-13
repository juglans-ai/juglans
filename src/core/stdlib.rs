//! Built-in standard library files, embedded at compile time via build.rs.
include!(concat!(env!("OUT_DIR"), "/stdlib_generated.rs"));
