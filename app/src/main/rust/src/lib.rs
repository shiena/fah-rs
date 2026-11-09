use std::{fs, path::Path};
#[cfg(target_os = "android")]
mod android;
#[cfg(any(
    target_os = "android",
    all(target_os = "windows", feature = "native-runtime")
))]
pub mod check;
pub mod pipeline;
pub mod runtime;
pub type Result<T> = std::result::Result<T, String>;
pub fn read_floats(path: impl AsRef<Path>) -> Result<Vec<f32>> {
    let bytes = fs::read(path.as_ref()).map_err(|e| format!("{}: {e}", path.as_ref().display()))?;
    if bytes.len() % 4 != 0 {
        return Err("Invalid float file".into());
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect())
}
pub fn write_floats(path: impl AsRef<Path>, values: &[f32]) -> Result<()> {
    let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    fs::write(path, bytes).map_err(|e| e.to_string())
}
