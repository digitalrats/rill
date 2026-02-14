use std::sync::Arc;
use std::thread;
use parking_lot::RwLock;
use tokio::sync::broadcast;
use crossbeam_channel::{unbounded, Sender, Receiver};

use midir::{MidiInput, MidiInputConnection, Ignore};

use crate::backend::{ControlBackend, BackendType, DeviceInfo, ControlEvent};
use crate::error::{ControlResult, ControlError};

// MIDI бэкенд работает в отдельном потоке, поэтому все структуры midir
// находятся в этом потоке и не влияют на Sync основной структуры
pub struct MidiBackend {
    name: String,
    // Каналы для коммуникации с MIDI потоком
    command_tx: Sender<MidiCommand>,
    event_tx: broadcast::Sender<ControlEvent>,
    event_rx: broadcast::Receiver<ControlEvent>,
    thread_handle: Option<thread::JoinHandle<()>>,
    is_running: Arc<RwLock<bool>>,
    devices: Arc<RwLock<Vec<DeviceInfo>>>,
}

enum MidiCommand {
    OpenPort(usize),
    OpenPortByName(String),
    Start,
    Stop,
    ListDevices,
}

impl MidiBackend {
    pub fn new(client_name: &str) -> ControlResult<Self> {
        let (command_tx, command_rx) = unbounded();
        let (event_tx, event_rx) = broadcast::channel(128);
        let is_running = Arc::new(RwLock::new(false));
        let devices = Arc::new(RwLock::new(Vec::new()));
        
        let thread_is_running = is_running.clone();
        let thread_devices = devices.clone();
        let thread_event_tx = event_tx.clone();
        let thread_client_name = client_name.to_string();
        
        // Запускаем поток для работы с MIDI
        let handle = thread::spawn(move || {
            run_midi_thread(
                command_rx,
                thread_event_tx,
                thread_is_running,
                thread_devices,
                thread_client_name,
            );
        });
        
        Ok(Self {
            name: client_name.to_string(),
            command_tx,
            event_tx,
            event_rx,
            thread_handle: Some(handle),
            is_running,
            devices,
        })
    }
    
    /// Открыть порт по индексу
    pub fn open_port(&self, port_index: usize) -> ControlResult<()> {
        self.command_tx.send(MidiCommand::OpenPort(port_index))
            .map_err(|_| ControlError::Channel)?;
        Ok(())
    }
    
    /// Открыть порт по имени
    pub fn open_port_by_name(&self, port_name: &str) -> ControlResult<()> {
        self.command_tx.send(MidiCommand::OpenPortByName(port_name.to_string()))
            .map_err(|_| ControlError::Channel)?;
        Ok(())
    }
    
    /// Открыть все доступные порты
    pub fn open_all_ports(&self) -> ControlResult<usize> {
        // В этой реализации открываем только первый порт
        self.open_port(0)?;
        Ok(1)
    }
}

// Функция, выполняющаяся в отдельном потоке
fn run_midi_thread(
    command_rx: Receiver<MidiCommand>,
    event_tx: broadcast::Sender<ControlEvent>,
    is_running: Arc<RwLock<bool>>,
    devices: Arc<RwLock<Vec<DeviceInfo>>>,
    client_name: String,
) {
    // Создаем MIDI вход (это делается в потоке)
    let mut midi_input = match MidiInput::new(&client_name) {
        Ok(input) => input,
        Err(e) => {
            eprintln!("Failed to create MIDI input: {}", e);
            return;
        }
    };
    
    midi_input.ignore(Ignore::None);
    
    // Получаем список портов
    let ports = midi_input.ports();
    let mut device_list = Vec::new();
    
    for (i, port) in ports.iter().enumerate() {
        if let Ok(name) = midi_input.port_name(port) {
            device_list.push(DeviceInfo {
                name,
                backend: BackendType::Midi,
                is_default: i == 0,
                input_ports: vec![format!("Port {}", i)],
                output_ports: Vec::new(),
            });
        }
    }
    
    *devices.write() = device_list;
    
    let mut connections: Vec<MidiInputConnection<()>> = Vec::new();
    
    // Основной цикл обработки команд
    while let Ok(cmd) = command_rx.recv() {
        match cmd {
            MidiCommand::OpenPort(index) => {
                // Получаем список портов текущего состояния
                let ports = midi_input.ports();
                if index >= ports.len() {
                    eprintln!("MIDI port {} not found", index);
                    continue;
                }
                
                let port = ports[index].clone();
                let port_name = match midi_input.port_name(&port) {
                    Ok(name) => name,
                    Err(e) => {
                        eprintln!("Failed to get port name: {}", e);
                        continue;
                    }
                };
                
                let tx = event_tx.clone();
                
                // ИСПРАВЛЕНИЕ: Создаем новый MidiInput для каждого соединения
                // Это решает проблему перемещения midi_input
                match MidiInput::new(&format!("{}_port_{}", client_name, index)) {
                    Ok(new_input) => {
                        match new_input.connect(
                            &port,
                            "kama-control",
                            move |_stamp, message, _| {
                                if message.len() >= 3 {
                                    let status = message[0];
                                    let channel = status & 0x0F;
                                    let msg_type = status & 0xF0;
                                    
                                    match msg_type {
                                        0x80 => { // Note Off
                                            let note = message[1];
                                            let _ = tx.send(ControlEvent::MidiNote {
                                                channel,
                                                note,
                                                velocity: 0,
                                                on: false,
                                            });
                                        }
                                        0x90 => { // Note On
                                            let note = message[1];
                                            let velocity = message[2];
                                            let _ = tx.send(ControlEvent::MidiNote {
                                                channel,
                                                note,
                                                velocity,
                                                on: velocity > 0,
                                            });
                                        }
                                        0xB0 => { // Control Change
                                            let controller = message[1];
                                            let value = message[2];
                                            let normalized = value as f32 / 127.0;
                                            let _ = tx.send(ControlEvent::MidiControl {
                                                channel,
                                                controller,
                                                value,
                                                normalized,
                                            });
                                        }
                                        _ => {
                                            let _ = tx.send(ControlEvent::Midi {
                                                channel,
                                                message: message.to_vec(),
                                            });
                                        }
                                    }
                                }
                            },
                            (),
                        ) {
                            Ok(conn) => {
                                println!("Opened MIDI port: {}", port_name);
                                connections.push(conn);
                            }
                            Err(e) => {
                                eprintln!("Failed to connect to MIDI port: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to create MIDI input for port {}: {}", index, e);
                    }
                }
            }
            
            MidiCommand::OpenPortByName(name) => {
                let ports = midi_input.ports();
                for (i, port) in ports.iter().enumerate() {
                    if let Ok(port_name) = midi_input.port_name(port) {
                        if port_name.contains(&name) {
                            // Отправляем команду открытия порта через тот же канал
                            // Но мы не можем использовать command_rx для отправки, поэтому создаем временный канал
                            let (tmp_tx, tmp_rx) = unbounded::<MidiCommand>();
                            let _ = tmp_tx.send(MidiCommand::OpenPort(i));
                            // Обрабатываем полученную команду рекурсивно
                            if let Ok(cmd) = tmp_rx.recv_timeout(std::time::Duration::from_millis(100)) {
                                // Рекурсивно обрабатываем команду
                                match cmd {
                                    MidiCommand::OpenPort(idx) => {
                                        // Вызываем ту же логику, но с новым индексом
                                        // Для простоты просто вызываем ту же функцию
                                        let tx = event_tx.clone();
                                        let port = ports[idx].clone();
                                        let port_name = port_name.clone();
                                        
                                        if let Ok(new_input) = MidiInput::new(&format!("{}_port_{}", client_name, idx)) {
                                            if let Ok(conn) = new_input.connect(
                                                &port,
                                                "kama-control",
                                                move |_stamp, message, _| {
                                                    // Та же логика обработки MIDI
                                                    if message.len() >= 3 {
                                                        let status = message[0];
                                                        let channel = status & 0x0F;
                                                        let msg_type = status & 0xF0;
                                                        
                                                        match msg_type {
                                                            0x80 => {
                                                                let note = message[1];
                                                                let _ = tx.send(ControlEvent::MidiNote {
                                                                    channel,
                                                                    note,
                                                                    velocity: 0,
                                                                    on: false,
                                                                });
                                                            }
                                                            0x90 => {
                                                                let note = message[1];
                                                                let velocity = message[2];
                                                                let _ = tx.send(ControlEvent::MidiNote {
                                                                    channel,
                                                                    note,
                                                                    velocity,
                                                                    on: velocity > 0,
                                                                });
                                                            }
                                                            0xB0 => {
                                                                let controller = message[1];
                                                                let value = message[2];
                                                                let normalized = value as f32 / 127.0;
                                                                let _ = tx.send(ControlEvent::MidiControl {
                                                                    channel,
                                                                    controller,
                                                                    value,
                                                                    normalized,
                                                                });
                                                            }
                                                            _ => {
                                                                let _ = tx.send(ControlEvent::Midi {
                                                                    channel,
                                                                    message: message.to_vec(),
                                                                });
                                                            }
                                                        }
                                                    }
                                                },
                                                (),
                                            ) {
                                                println!("Opened MIDI port by name: {}", port_name);
                                                connections.push(conn);
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            break;
                        }
                    }
                }
            }
            
            MidiCommand::Start => {
                *is_running.write() = true;
            }
            
            MidiCommand::Stop => {
                *is_running.write() = false;
                connections.clear();
            }
            
            MidiCommand::ListDevices => {
                let ports = midi_input.ports();
                let mut device_list = Vec::new();
                
                for (i, port) in ports.iter().enumerate() {
                    if let Ok(name) = midi_input.port_name(port) {
                        device_list.push(DeviceInfo {
                            name,
                            backend: BackendType::Midi,
                            is_default: i == 0,
                            input_ports: vec![format!("Port {}", i)],
                            output_ports: Vec::new(),
                        });
                    }
                }
                
                *devices.write() = device_list;
            }
        }
    }
}

impl ControlBackend for MidiBackend {
    fn name(&self) -> &'static str {
        "MIDI"
    }
    
    fn backend_type(&self) -> BackendType {
        BackendType::Midi
    }
    
    fn init(&mut self) -> ControlResult<()> {
        Ok(())
    }
    
    fn start(&mut self) -> ControlResult<()> {
        self.command_tx.send(MidiCommand::Start)
            .map_err(|_| ControlError::Channel)?;
        Ok(())
    }
    
    fn stop(&mut self) -> ControlResult<()> {
        self.command_tx.send(MidiCommand::Stop)
            .map_err(|_| ControlError::Channel)?;
        Ok(())
    }
    
    fn subscribe(&self) -> broadcast::Receiver<ControlEvent> {
        self.event_tx.subscribe()
    }
    
    fn list_devices(&self) -> Vec<DeviceInfo> {
        // Обновляем список устройств
        let _ = self.command_tx.send(MidiCommand::ListDevices);
        // Даем время на обновление (в реальном коде лучше использовать канал с ответом)
        thread::sleep(std::time::Duration::from_millis(10));
        self.devices.read().clone()
    }
    
    fn is_available(&self) -> bool {
        true
    }
}

impl Drop for MidiBackend {
    fn drop(&mut self) {
        let _ = self.command_tx.send(MidiCommand::Stop);
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }
}