use std::fmt;
use std::sync::Arc;

use super::ring::Ring;

#[derive(Debug)]
pub enum AudioError {
    NoDevice,
    Config(String),
    Device(String),
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudioError::NoDevice => write!(f, "no default output device"),
            AudioError::Config(m) => write!(f, "unusable output config: {m}"),
            AudioError::Device(m) => write!(f, "audio device: {m}"),
        }
    }
}

impl std::error::Error for AudioError {}

pub trait AudioSink: Send {
    fn open(&mut self, sample_rate: u32) -> Result<(), AudioError>;
    fn ring(&self) -> Arc<Ring>;
    fn close(&mut self) {}
    /// Closes without waiting for the device to be let go, for when another
    /// program is about to start and will not want it for a while yet.
    fn close_soon(&mut self) {
        self.close();
    }
    fn hold_open(&self, _held: bool) {}
}
