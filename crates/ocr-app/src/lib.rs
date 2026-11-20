//! Application layer shared by the CLI and GUI: preflight, job runner, doctor
//! and model download.

pub mod doctor;
pub mod download;
pub mod job;
pub mod runtime;
pub mod setup;

pub use doctor::{doctor, print_doctor, Check, DoctorReport};
pub use download::{
    download_models, download_models_cancellable, human_bytes, DownloadProgress, MODEL_FILES,
};
pub use job::{preflight, run_job, JobOptions, JobReport, PreparedJob};
/// Speed-profile constants shared by the CLI and GUI.
pub use ocr_engine::{
    DEFAULT_IMAGE_MAX_TOKENS, DEFAULT_PARALLEL_PAGES, FAST_IMAGE_MAX_TOKENS, MAX_PARALLEL_PAGES,
};
pub use runtime::{
    download_runtime, runtime_assets, runtime_assets_for, runtime_download_mb, verify_runtime,
    RuntimeAsset, RuntimeComponent, RuntimeInstall, RuntimeProgress,
};
pub use setup::{
    check_model_status, model_status, pdfium_for_runtime, runtime_pdfium_candidate, runtime_status,
    ModelStatus, PdfiumSource, RuntimeStatus,
};

pub fn device_name(device: ocr_core::Device) -> &'static str {
    match device {
        ocr_core::Device::Auto => "auto",
        ocr_core::Device::Gpu => "gpu",
        ocr_core::Device::Cpu => "cpu",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_names_are_stable() {
        assert_eq!(device_name(ocr_core::Device::Auto), "auto");
        assert_eq!(device_name(ocr_core::Device::Gpu), "gpu");
        assert_eq!(device_name(ocr_core::Device::Cpu), "cpu");
    }
}
