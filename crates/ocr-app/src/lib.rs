//! Application layer shared by the CLI and GUI: preflight, job runner, doctor
//! and model download.

pub mod doctor;
pub mod download;
pub mod job;

pub use doctor::{doctor, print_doctor, Check, DoctorReport};
pub use download::{download_models, human_bytes, DownloadProgress, MODEL_FILES};
pub use job::{preflight, run_job, JobOptions, JobReport, PreparedJob};

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
