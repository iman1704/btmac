//! Apple Silicon only, IOReport/SMC sourcing

#[cfg(target_os = "macos")]
pub mod gpu;
