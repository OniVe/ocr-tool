//! Chandra OCR inference through a local `llama-server` sidecar.

mod protocol;
mod server;

pub use protocol::{build_request, parse_chat_response};
pub use server::{
    inspect_candidates, CandidateInfo, EngineConfig, OcrServer, DEFAULT_CONTEXT_SIZE,
    DEFAULT_MAX_TOKENS, LLAMA_ENV, REQUEST_TIMEOUT, STARTUP_TIMEOUT,
};
