pub use fah_core::pipeline::*;
use fah_core::Result;
pub fn create(directory: &str, gpu: bool, half: bool) -> Result<Pipeline> {
    let path = directory.to_owned();
    Pipeline::new(
        directory,
        gpu,
        half,
        Box::new(move |gpu, half| {
            Ok(Box::new(crate::runtime::Runtime::new(
                &path, gpu, half, true,
            )?))
        }),
    )
    .map(|pipeline| pipeline.with_gpu_backend("vulkan"))
}
