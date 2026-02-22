use kama_io::{
    AudioConfig, AudioEngine, AudioProcessor, AudioBackend,
    backends::NullBackend,
    processor::GainProcessor,
};

mod mocks;
use mocks::test_config;

#[test]
fn test_null_backend_basic() {
    let config = test_config();
    let mut backend = NullBackend::new(config);
    
    assert_eq!(backend.backend_type().name(), "Null");
    assert!(backend.init().is_ok());
    assert!(backend.start().is_ok());
    
    let mut buf = vec![0.0; 256];
    let read = backend.read(&mut buf).unwrap();
    assert_eq!(read, 256);
    assert!(buf.iter().all(|&x| x == 0.0));
    
    let write_buf = vec![0.5; 256];
    let written = backend.write(&write_buf).unwrap();
    assert_eq!(written, 256);
    
    assert!(backend.stop().is_ok());
    assert_eq!(backend.xruns(), 0);
}

#[test]
fn test_audio_engine_with_null_backend() {
    let config = test_config();
    
    let backend = NullBackend::new(config.clone());
    let processor = GainProcessor::new(1.0);
    
    let mut engine = AudioEngine::new(backend, processor);
    
    assert_eq!(engine.state(), kama_io::EngineState::Stopped);
    
    engine.start().unwrap();
    assert_eq!(engine.state(), kama_io::EngineState::Running);
    
    std::thread::sleep(std::time::Duration::from_millis(10));
    
    engine.stop().unwrap();
    assert_eq!(engine.state(), kama_io::EngineState::Stopped);
    
    assert_eq!(engine.xruns(), 0);
}

#[test]
fn test_audio_engine_update_processor() {
    let config = test_config();
    
    let backend = NullBackend::new(config.clone());
    let processor = GainProcessor::new(1.0);
    
    let mut engine = AudioEngine::new(backend, processor);
    
    engine.start().unwrap();
    
    engine.update_processor(|p: &mut GainProcessor| {
        p.set_gain(2.0);
    }).unwrap();
    
    std::thread::sleep(std::time::Duration::from_millis(10));
    
    engine.stop().unwrap();
}