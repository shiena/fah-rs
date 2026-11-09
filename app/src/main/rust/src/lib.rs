pub use fah_core::{read_floats, write_floats, Result};
pub use fah_ncnn::{check, pipeline, runtime};
#[cfg(target_os = "android")]
mod android;
