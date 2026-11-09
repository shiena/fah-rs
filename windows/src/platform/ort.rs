use anyhow::{Context, Result, ensure};
use fah_api::Backend;
use libloading::{
    Library,
    os::windows::{LOAD_WITH_ALTERED_SEARCH_PATH, Library as WindowsLibrary},
};
use std::{ffi::OsStr, os::windows::ffi::OsStrExt, path::PathBuf};
pub fn load(path: &std::path::Path) -> Result<Library> {
    Ok(unsafe { WindowsLibrary::load_with_flags(path, LOAD_WITH_ALTERED_SEARCH_PATH) }?.into())
}
pub fn wide(path: &OsStr) -> Vec<u16> {
    path.encode_wide().chain(Some(0)).collect()
}
pub fn runtime_path(backend: Backend) -> Result<PathBuf> {
    let key = if backend == Backend::Cuda {
        "FAH_ORT_CUDA"
    } else {
        "FAH_ORT_DML"
    };
    let adjacent = std::env::current_exe()?
        .parent()
        .unwrap()
        .join("runtimes")
        .join(if backend == Backend::Cuda {
            "cuda"
        } else {
            "directml"
        })
        .join("onnxruntime.dll");
    let development = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../third_party")
        .join(if backend == Backend::Cuda {
            "ort-cuda13-1.31.1/onnxruntime-win-x64-gpu_cuda13-1.31.1/lib/onnxruntime.dll"
        } else {
            "ort-directml-1.24.4/runtimes/win-x64/native/onnxruntime.dll"
        });
    let path = std::env::var_os(key).map(PathBuf::from).unwrap_or_else(|| {
        if adjacent.is_file() || !development.is_file() {
            adjacent
        } else {
            development
        }
    });
    path.canonicalize().with_context(|| {
        format!(
            "{} is missing. Set {key} to onnxruntime.dll; see windows/README.md",
            path.display()
        )
    })
}
const CUDA_DEPENDENCIES: &[&str] = &[
    "cudart64_13.dll",
    "cublas64_13.dll",
    "cublasLt64_13.dll",
    "cudnn64_9.dll",
    "nvcuda.dll",
];

fn missing_libraries(names: &[&str], directories: &[PathBuf]) -> Vec<String> {
    names
        .iter()
        .filter(|name| !directories.iter().any(|dir| dir.join(name).is_file()))
        .map(|name| (*name).to_owned())
        .collect()
}

/// Check files before loading CUDA or constructing either model.
pub fn cuda_libraries() -> Result<()> {
    let runtime = runtime_path(Backend::Cuda)?;
    let runtime_dir = runtime.parent().unwrap().to_path_buf();
    let mut missing = missing_libraries(
        &[
            "onnxruntime_providers_cuda.dll",
            "onnxruntime_providers_shared.dll",
        ],
        std::slice::from_ref(&runtime_dir),
    );
    let mut directories = vec![
        runtime_dir,
        std::env::current_exe()?.parent().unwrap().to_path_buf(),
    ];
    if let Some(windows) = std::env::var_os("SystemRoot") {
        directories.push(PathBuf::from(windows).join("System32"));
    }
    if let Some(path) = std::env::var_os("PATH") {
        directories.extend(std::env::split_paths(&path).filter(|p| p.is_absolute()));
    }
    missing.extend(missing_libraries(CUDA_DEPENDENCIES, &directories));
    ensure!(
        missing.is_empty(),
        "CUDA libraries missing: {}. See windows/README.md",
        missing.join(", ")
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cuda_requires_every_dependency_across_search_directories() {
        let root = std::env::temp_dir().join(format!(
            "fah-cuda-probe-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let second = root.join("second");
        std::fs::create_dir(&second).unwrap();
        let dirs = [root.clone(), second.clone()];
        for name in CUDA_DEPENDENCIES
            .iter()
            .filter(|name| **name != "cudnn64_9.dll")
        {
            std::fs::write(root.join(name), []).unwrap();
        }
        assert_eq!(
            missing_libraries(CUDA_DEPENDENCIES, &dirs),
            vec!["cudnn64_9.dll"]
        );
        std::fs::write(second.join("cudnn64_9.dll"), []).unwrap();
        assert!(missing_libraries(CUDA_DEPENDENCIES, &dirs).is_empty());
        for name in CUDA_DEPENDENCIES
            .iter()
            .filter(|name| **name != "cudnn64_9.dll")
        {
            std::fs::remove_file(root.join(name)).unwrap();
        }
        std::fs::remove_file(second.join("cudnn64_9.dll")).unwrap();
        std::fs::remove_dir(second).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
