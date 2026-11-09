use anyhow::{Result, bail};
use fah_api::{Backend, Frame, InferenceFactory, Output, TrackingSession};
use fah_core::pipeline::Pipeline;
use std::{path::Path, sync::Arc};

/// Engine-independent entry point for hosts that supply their own frame loop.
pub struct Session {
    pipeline: Pipeline,
    backend: Backend,
    requested: Backend,
    warning: String,
}
impl Session {
    pub fn open(
        models: &Path,
        requested: Backend,
        factory: Arc<dyn InferenceFactory>,
    ) -> Result<Self> {
        let choices: &[Backend] = if requested == Backend::Auto {
            &[Backend::Cuda, Backend::DirectMl, Backend::Cpu]
        } else {
            std::slice::from_ref(&requested)
        };
        let capabilities = factory.capabilities();
        let mut failures = Vec::new();
        for &backend in choices {
            if requested == Backend::Auto && backend == Backend::Cuda && !capabilities.cuda {
                continue;
            }
            let path = models.to_path_buf();
            let provider = factory.clone();
            let pipeline = Pipeline::new(
                &models.to_string_lossy(),
                backend != Backend::Cpu,
                false,
                Box::new(move |gpu, _| {
                    let result = if gpu {
                        provider.create(&path, backend)
                    } else {
                        provider.cpu_fallback(&path, backend)
                    };
                    result.map_err(|error| format!("{error:#}"))
                }),
            );
            match pipeline {
                Ok(pipeline) => {
                    return Ok(Self {
                        pipeline,
                        backend,
                        requested,
                        warning: failures.join("\n"),
                    });
                }
                Err(error) => failures.push(format!("{}: {error}", backend.name())),
            }
        }
        bail!("{}", failures.join("\n"))
    }
}
impl TrackingSession for Session {
    fn process(&mut self, frame: Frame, reset: bool) -> Result<Output> {
        let Frame { image, captured } = frame;
        let json = self
            .pipeline
            .process(
                image.as_raw(),
                image.width() as usize,
                image.height() as usize,
                image.width() as usize * 4,
                reset,
            )
            .map_err(anyhow::Error::msg)?;
        let mut result: serde_json::Value = serde_json::from_str(&json)?;
        if result["cpu_fallback"] == true {
            self.backend = Backend::Cpu;
        }
        result["backend"] = self.backend.name().into();
        result["requested_backend"] = self.requested.name().into();
        result["initialization_warning"] = self.warning.clone().into();
        result["initialization_fallback"] = (!self.warning.is_empty()).into();
        Ok(Output {
            id: 0,
            image,
            result,
            latency_ms: captured.elapsed().as_secs_f64() * 1000.,
        })
    }
    fn backend(&self) -> Backend {
        self.backend
    }
    fn warning(&self) -> &str {
        &self.warning
    }
}
