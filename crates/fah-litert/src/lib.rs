//! Owned CPU-only LiteRT 2.3 C API model, with no Python or GUI dependency.
use anyhow::{ensure, Context, Result};
use libloading::Library;
use std::{
    ffi::{c_char, c_void, CStr, CString},
    path::Path,
    ptr,
};

type Handle = *mut c_void;
type Status = i32;
// Public LiteRT 2.3 ABI: litert/c/litert_layout.h and litert_model_types.h.
#[repr(C)]
#[derive(Default)]
struct RankedType {
    element: i32,
    rank_flags: u32,
    dimensions: [i32; 8],
    strides: [u32; 8],
}
const _: () = assert!(std::mem::size_of::<RankedType>() == 72);

macro_rules! api {
    ($($field:ident: $symbol:literal ($($arg:ty),*) -> $ret:ty;)*) => {
        struct Api { $($field: unsafe extern "C" fn($($arg),*) -> $ret,)* _library: Library }
        impl Api {
            unsafe fn load(path: &Path) -> Result<Self> {
                let library = Library::new(path).with_context(|| format!("Load LiteRT: {}", path.display()))?;
                Ok(Self { $($field: *library.get(concat!($symbol, "\0").as_bytes())?,)* _library: library })
            }
        }
    };
}
api! {
    create_env: "LiteRtCreateEnvironment" (i32, *const c_void, *mut Handle) -> Status;
    destroy_env: "LiteRtDestroyEnvironment" (Handle) -> ();
    create_model: "LiteRtCreateModelFromFile" (Handle, *const c_char, *mut Handle) -> Status;
    destroy_model: "LiteRtDestroyModel" (Handle) -> ();
    create_options: "LiteRtCreateOptions" (*mut Handle) -> Status;
    destroy_options: "LiteRtDestroyOptions" (Handle) -> ();
    set_hardware: "LiteRtSetOptionsHardwareAccelerators" (Handle, i32) -> Status;
    create_opaque: "LiteRtCreateOpaqueOptions" (*const c_char, Handle, Option<unsafe extern "C" fn(Handle)>, *mut Handle) -> Status;
    destroy_opaque: "LiteRtDestroyOpaqueOptions" (Handle) -> ();
    add_opaque: "LiteRtAddOpaqueOptions" (Handle, Handle) -> Status;
    compile: "LiteRtCreateCompiledModel" (Handle, Handle, Handle, *mut Handle) -> Status;
    destroy_compiled: "LiteRtDestroyCompiledModel" (Handle) -> ();
    subgraph: "LiteRtGetModelSubgraph" (Handle, usize, *mut Handle) -> Status;
    input_count: "LiteRtGetNumSubgraphInputs" (Handle, *mut usize) -> Status;
    output_count: "LiteRtGetNumSubgraphOutputs" (Handle, *mut usize) -> Status;
    input: "LiteRtGetSubgraphInput" (Handle, usize, *mut Handle) -> Status;
    output: "LiteRtGetSubgraphOutput" (Handle, usize, *mut Handle) -> Status;
    tensor_name: "LiteRtGetTensorName" (Handle, *mut *const c_char) -> Status;
    tensor_type: "LiteRtGetRankedTensorType" (Handle, *mut RankedType) -> Status;
    input_requirements: "LiteRtGetCompiledModelInputBufferRequirements" (Handle, usize, usize, *mut Handle) -> Status;
    output_requirements: "LiteRtGetCompiledModelOutputBufferRequirements" (Handle, usize, usize, *mut Handle) -> Status;
    create_buffer: "LiteRtCreateManagedTensorBufferFromRequirements" (Handle, *const RankedType, Handle, *mut Handle) -> Status;
    destroy_buffer: "LiteRtDestroyTensorBuffer" (Handle) -> ();
    lock: "LiteRtLockTensorBuffer" (Handle, *mut Handle, i32) -> Status;
    unlock: "LiteRtUnlockTensorBuffer" (Handle) -> Status;
    run: "LiteRtRunCompiledModel" (Handle, usize, usize, *mut Handle, usize, *mut Handle) -> Status;
}
fn check(status: Status, operation: &str) -> Result<()> {
    ensure!(status == 0, "LiteRT {operation} failed (status {status})");
    Ok(())
}
unsafe extern "C" fn static_payload(_: Handle) {}

/// Expected FP32 tensor layout, checked before allocating inference buffers.
pub struct TensorSpec<'a> {
    pub name: &'a str,
    pub shape: &'a [i32],
}

pub struct Model {
    api: Api,
    environment: Handle,
    model: Handle,
    options: Handle,
    opaque: Handle,
    compiled: Handle,
    inputs: Vec<Handle>,
    outputs: Vec<Handle>,
    input_lengths: Vec<usize>,
    output_lengths: Vec<usize>,
}
// Ownership can move between workers; all inference requires exclusive &mut self.
unsafe impl Send for Model {}
impl Model {
    pub fn open(
        library: &Path,
        path: &Path,
        inputs: &[TensorSpec<'_>],
        outputs: &[TensorSpec<'_>],
    ) -> Result<Self> {
        let file = CString::new(path.to_str().context("LiteRT model path must be UTF-8")?)?;
        let mut this = Self {
            api: unsafe { Api::load(library)? },
            environment: ptr::null_mut(),
            model: ptr::null_mut(),
            options: ptr::null_mut(),
            opaque: ptr::null_mut(),
            compiled: ptr::null_mut(),
            inputs: vec![],
            outputs: vec![],
            input_lengths: vec![],
            output_lengths: vec![],
        };
        unsafe {
            check(
                (this.api.create_env)(0, ptr::null(), &mut this.environment),
                "create environment",
            )?;
            check(
                (this.api.create_model)(this.environment, file.as_ptr(), &mut this.model),
                "load model",
            )?;
            check(
                (this.api.create_options)(&mut this.options),
                "create options",
            )?;
            check((this.api.set_hardware)(this.options, 1), "select CPU")?;
            // CPU options use the documented v2.3 TOML payload, owned by Options.
            check(
                (this.api.create_opaque)(
                    c"xnnpack".as_ptr(),
                    c"num_threads = 4\nkernel_mode = \"delegate\"\n".as_ptr() as Handle,
                    Some(static_payload),
                    &mut this.opaque,
                ),
                "create CPU options",
            )?;
            check(
                (this.api.add_opaque)(this.options, this.opaque),
                "set CPU options",
            )?;
            this.opaque = ptr::null_mut();
            check(
                (this.api.compile)(
                    this.environment,
                    this.model,
                    this.options,
                    &mut this.compiled,
                ),
                "compile CPU model",
            )?;
            this.buffers(inputs, true)?;
            this.buffers(outputs, false)?;
        }
        Ok(this)
    }
    unsafe fn buffers(&mut self, specs: &[TensorSpec<'_>], input: bool) -> Result<()> {
        let mut graph = ptr::null_mut();
        check(
            (self.api.subgraph)(self.model, 0, &mut graph),
            "get main subgraph",
        )?;
        let mut count = 0;
        let count_fn = if input {
            self.api.input_count
        } else {
            self.api.output_count
        };
        check(count_fn(graph, &mut count), "get tensor count")?;
        ensure!(
            count == specs.len(),
            "Unexpected LiteRT tensor count: {count}"
        );
        for (index, spec) in specs.iter().enumerate() {
            let mut tensor = ptr::null_mut();
            let get = if input {
                self.api.input
            } else {
                self.api.output
            };
            check(get(graph, index, &mut tensor), "get tensor")?;
            let mut ty = RankedType::default();
            check((self.api.tensor_type)(tensor, &mut ty), "get tensor type")?;
            ensure!(
                ty.element == 1
                    && ty.rank_flags == spec.shape.len() as u32
                    && spec.shape.len() <= 8
                    && &ty.dimensions[..spec.shape.len()] == spec.shape,
                "Unexpected LiteRT shape/type for {}",
                spec.name
            );
            let mut name = ptr::null();
            check((self.api.tensor_name)(tensor, &mut name), "get tensor name")?;
            ensure!(
                !name.is_null() && CStr::from_ptr(name).to_bytes() == spec.name.as_bytes(),
                "Unexpected LiteRT tensor order for {}",
                spec.name
            );
            let length = spec.shape.iter().try_fold(1usize, |n, &d| {
                ensure!(d > 0, "Dynamic or empty LiteRT dimensions are unsupported");
                n.checked_mul(d as usize)
                    .context("LiteRT tensor size overflow")
            })?;
            let mut requirements = ptr::null_mut();
            let get = if input {
                self.api.input_requirements
            } else {
                self.api.output_requirements
            };
            check(
                get(self.compiled, 0, index, &mut requirements),
                "get buffer requirements",
            )?;
            let mut buffer = ptr::null_mut();
            check(
                (self.api.create_buffer)(self.environment, &ty, requirements, &mut buffer),
                "allocate buffer",
            )?;
            if input {
                self.inputs.push(buffer);
                self.input_lengths.push(length);
            } else {
                self.outputs.push(buffer);
                self.output_lengths.push(length);
            }
        }
        Ok(())
    }
    pub fn run(&mut self, inputs: &[&[f32]]) -> Result<Vec<Vec<f32>>> {
        ensure!(
            inputs.len() == self.inputs.len(),
            "Invalid LiteRT input count"
        );
        ensure!(
            inputs
                .iter()
                .zip(&self.input_lengths)
                .all(|(data, size)| data.len() == *size),
            "Invalid LiteRT input length"
        );
        unsafe {
            for (data, &buffer) in inputs.iter().zip(&self.inputs) {
                let lock = Locked::new(&self.api, buffer, 1)?;
                ptr::copy_nonoverlapping(data.as_ptr(), lock.data.cast::<f32>(), data.len());
                lock.finish()?;
            }
            check(
                (self.api.run)(
                    self.compiled,
                    0,
                    self.inputs.len(),
                    self.inputs.as_mut_ptr(),
                    self.outputs.len(),
                    self.outputs.as_mut_ptr(),
                ),
                "invoke",
            )?;
            let mut outputs = Vec::with_capacity(self.outputs.len());
            for (&buffer, &length) in self.outputs.iter().zip(&self.output_lengths) {
                let mut data = vec![0.; length];
                let lock = Locked::new(&self.api, buffer, 0)?;
                ptr::copy_nonoverlapping(lock.data.cast::<f32>(), data.as_mut_ptr(), length);
                lock.finish()?;
                ensure!(
                    data.iter().all(|v| v.is_finite()),
                    "Non-finite LiteRT output"
                );
                outputs.push(data);
            }
            Ok(outputs)
        }
    }
}
struct Locked<'a> {
    api: &'a Api,
    buffer: Handle,
    data: Handle,
}
impl<'a> Locked<'a> {
    unsafe fn new(api: &'a Api, buffer: Handle, mode: i32) -> Result<Self> {
        let mut data = ptr::null_mut();
        check((api.lock)(buffer, &mut data, mode), "lock buffer")?;
        let lock = Self { api, buffer, data };
        ensure!(!data.is_null(), "LiteRT returned null buffer storage");
        Ok(lock)
    }
    unsafe fn finish(mut self) -> Result<()> {
        let status = (self.api.unlock)(self.buffer);
        self.buffer = ptr::null_mut();
        check(status, "unlock buffer")
    }
}
impl Drop for Locked<'_> {
    fn drop(&mut self) {
        if !self.buffer.is_null() {
            unsafe {
                (self.api.unlock)(self.buffer);
            }
        }
    }
}
impl Drop for Model {
    fn drop(&mut self) {
        unsafe {
            for &buffer in self.inputs.iter().chain(&self.outputs) {
                (self.api.destroy_buffer)(buffer);
            }
            if !self.compiled.is_null() {
                (self.api.destroy_compiled)(self.compiled);
            }
            if !self.opaque.is_null() {
                (self.api.destroy_opaque)(self.opaque);
            }
            if !self.options.is_null() {
                (self.api.destroy_options)(self.options);
            }
            if !self.model.is_null() {
                (self.api.destroy_model)(self.model);
            }
            if !self.environment.is_null() {
                (self.api.destroy_env)(self.environment);
            }
        }
    }
}
