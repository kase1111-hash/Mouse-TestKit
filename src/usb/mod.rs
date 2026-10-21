//! USB connection details (used by the CLI menu on Linux and Windows).

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub mod conflicts;
