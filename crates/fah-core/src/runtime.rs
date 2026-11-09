use crate::Result;
#[derive(Clone, Debug)]
pub struct Tensor {
    pub w: usize,
    pub h: usize,
    pub c: usize,
    pub data: Vec<f32>,
}
impl Tensor {
    pub fn new(w: usize, h: usize, c: usize, data: Vec<f32>) -> Self {
        Self { w, h, c, data }
    }
}
pub trait Inference {
    fn detect(&mut self, image: &Tensor) -> Result<Tensor>;
    fn pose(&mut self, image: &Tensor, rays: &Tensor, condition: &Tensor) -> Result<Vec<Tensor>>;
}
