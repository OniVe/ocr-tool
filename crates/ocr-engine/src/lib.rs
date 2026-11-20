//! Chandra OCR inference through a local `llama-server` sidecar.

mod protocol;
mod server;

pub use protocol::{build_request, parse_chat_response};
pub use server::{
    inspect_candidates, server_tuning, CandidateInfo, EngineConfig, OcrServer, ServerTuning,
    DEFAULT_CONTEXT_SIZE, DEFAULT_IMAGE_MAX_TOKENS, DEFAULT_MAX_TOKENS, DEFAULT_PARALLEL_PAGES,
    FAST_IMAGE_MAX_TOKENS, IMAGE_MAX_TOKENS_ENV, LLAMA_ENV, MAX_PARALLEL_PAGES,
    PARALLEL_CONTEXT_SIZE, REQUEST_TIMEOUT, STARTUP_TIMEOUT,
};
