use thiserror::Error;

#[derive(Error, Debug)]
pub enum ControlError {
    #[error("Backend error: {0}")]
    Backend(String),
    
    #[error("Device not found: {0}")]
    DeviceNotFound(String),
    
    #[error("MIDI error: {0}")]
    Midi(String),
    
    #[error("HID error: {0}")]
    Hid(String),
    
    #[error("OSC error: {0}")]
    Osc(String),
    
    #[error("Mapping error: {0}")]
    Mapping(String),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Channel error")]
    Channel,
}

pub type ControlResult<T> = Result<T, ControlError>;