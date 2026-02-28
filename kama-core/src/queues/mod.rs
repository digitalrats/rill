//! # Неблокирующие очереди
//!
//! Фундаментальный механизм коммуникации между компонентами Kama Audio.
//! Очереди обеспечивают надежную, неблокирующую передачу команд и данных,
//! не требуя от компонентов знать, кто находится на другом конце.
//!
//! ## Философия
//!
//! Очереди — это **законы природы** в мире Kama Audio. Компоненты знают,
//! что очередь существует, могут в неё писать и читать из неё, но не знают
//! и не должны знать, какие компоненты находятся по ту сторону. Это создает
//! слабую связанность и позволяет легко заменять и модифицировать компоненты.
//!
//! ## Два типа очередей
//!
//! - **CommandQueue<T>** — для команд (типобезопасная MPMC очередь)
//! - **TelemetryQueue** — специализированная очередь для телеметрии
//!
//! ## Наблюдатель
//!
//! **MicroControlObserver** — всевидящее око, следящее за нарушениями законов
//! природы (микро-контролем). Он никогда не вмешивается, только наблюдает и
//! записывает.
//!
//! ## Пример
//!
//! ```rust
//! use kama_core::queues::*;
//! use kama_core::traits::*;
//! use crossbeam_channel::unbounded;
//!
//! // Создаем очередь команд
//! let cmd_queue: CommandQueue<CommandEnum> = CommandQueue::new("audio-control");
//! 
//! // ИСПРАВЛЕНО: создаем идентификаторы перед использованием
//! let node = NodeId(1);
//! let port = PortId::control_in(node, 0);
//! let param = ParameterId::new("gain").unwrap();
//! 
//! // Где-то в мире автоматов
//! let cmd = SetParameter::new(port, param, 0.5, SignalSource::Automaton("lfo".into()));
//! cmd_queue.send(CommandEnum::SetParameter(cmd)).unwrap();
//! 
//! // Где-то в звуковом мире
//! while let Ok(cmd_enum) = cmd_queue.try_recv() {
//!     if let CommandEnum::SetParameter(cmd) = cmd_enum {
//!         println!("Применяем {} = {}", cmd.parameter, cmd.value);
//!     }
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Фундаментальный механизм коммуникации между компонентами Kama Audio.

mod command;
mod telemetry;
mod signal;
mod observer;
mod error;

pub use command::{
    CommandQueue, Command as CommandTrait, OverflowPolicy,
    QueueIter, QueueStats,
};
pub use telemetry::{
    Telemetry, TelemetryKind, TelemetryQueue, TelemetryQueueExt,
};
pub use signal::{
    CommandEnum, SetParameter, SignalSource,
    AutomatonCommand, SensorCommand, ServoCommand,
    CommandType, ToCommand, FromCommand,
};
pub use observer::{
    MicroControlObserver, MicroControlPermit,
    OperationGuard, Violation, ComponentStats, SandboxSummary,
};
pub use error::{QueueError, QueueResult};

/// Префикс для удобного импорта
pub mod prelude {
    pub use super::{
        CommandQueue, TelemetryQueue, Telemetry,
        CommandEnum, SetParameter, SignalSource,
        MicroControlObserver, MicroControlPermit,
        QueueError, QueueResult,
    };
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{NodeId, ParameterId, PortId};

    fn dummy_port() -> PortId {
        PortId::control_in(NodeId(1), 0)
    }

    fn dummy_param() -> ParameterId {
        ParameterId::new("test").unwrap()
    }

    #[test]
    fn test_command_queue_basic() {
        let queue: CommandQueue<SetParameter> = CommandQueue::new("test");
        let port = dummy_port();
        let param = dummy_param();

        let cmd = SetParameter::new(port, param, 0.5, SignalSource::Manual);
        queue.send(cmd).unwrap();

        assert_eq!(queue.len(), 1);
        assert!(!queue.is_empty());

        let received = queue.try_recv().unwrap();
        assert_eq!(received.value, 0.5);
        assert_eq!(queue.len(), 0);
        assert!(queue.is_empty());
    }

    #[test]
    fn test_command_queue_bounded() {
        let queue: CommandQueue<SetParameter> = CommandQueue::with_capacity("test", 2);
        let port = dummy_port();
        let param = dummy_param();

        let cmd1 = SetParameter::new(port, param.clone(), 0.1, SignalSource::Manual);
        let cmd2 = SetParameter::new(port, param.clone(), 0.2, SignalSource::Manual);
        let cmd3 = SetParameter::new(port, param.clone(), 0.3, SignalSource::Manual);

        queue.send(cmd1).unwrap();
        queue.send(cmd2).unwrap();
        
        // Третья должна вернуть ошибку "Full"
        assert!(queue.send(cmd3).is_err());
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn test_command_queue_with_policy_drop_newest() {
        let queue: CommandQueue<SetParameter> = CommandQueue::with_policy(
            "test", 2, OverflowPolicy::DropNewest
        );
        let port = dummy_port();
        let param = dummy_param();

        let cmd1 = SetParameter::new(port, param.clone(), 0.1, SignalSource::Manual);
        let cmd2 = SetParameter::new(port, param.clone(), 0.2, SignalSource::Manual);
        let cmd3 = SetParameter::new(port, param.clone(), 0.3, SignalSource::Manual);

        queue.send(cmd1).unwrap();
        queue.send(cmd2).unwrap();
        
        // DropNewest должна "проглотить" третью команду без ошибки
        queue.send(cmd3).unwrap();
        
        // В очереди должны остаться первые две
        assert_eq!(queue.len(), 2);
        
        let received1 = queue.try_recv().unwrap();
        assert_eq!(received1.value, 0.1);
        
        let received2 = queue.try_recv().unwrap();
        assert_eq!(received2.value, 0.2);
        
        assert!(queue.try_recv().is_err());
    }

    #[test]
    fn test_command_queue_with_policy_drop_oldest() {
        let queue: CommandQueue<SetParameter> = CommandQueue::with_policy(
            "test", 2, OverflowPolicy::DropOldest
        );
        let port = dummy_port();
        let param = dummy_param();

        let cmd1 = SetParameter::new(port, param.clone(), 0.1, SignalSource::Manual);
        let cmd2 = SetParameter::new(port, param.clone(), 0.2, SignalSource::Manual);
        let cmd3 = SetParameter::new(port, param.clone(), 0.3, SignalSource::Manual);

        queue.send(cmd1).unwrap();
        queue.send(cmd2).unwrap();
        
        // DropOldest должна вытеснить самую старую (0.1)
        queue.send(cmd3).unwrap();
        
        // В очереди должны остаться 0.2 и 0.3
        assert_eq!(queue.len(), 2);
        
        let received1 = queue.try_recv().unwrap();
        assert_eq!(received1.value, 0.2);
        
        let received2 = queue.try_recv().unwrap();
        assert_eq!(received2.value, 0.3);
        
        assert!(queue.try_recv().is_err());
    }

    #[test]
    fn test_telemetry_queue() {
        let queue = TelemetryQueue::new("test");
        let port = dummy_port();

        queue.send_peak(port, 0.8).unwrap();
        queue.send_event("test", "event", vec![1.0, 2.0]).unwrap();

        assert_eq!(queue.len(), 2);

        let received = queue.try_recv().unwrap();
        match received {
            Telemetry::Peak { value, .. } => {
                assert!((value - 0.8).abs() < 0.001);
            }
            _ => panic!("Expected Peak telemetry"),
        }

        let received = queue.try_recv().unwrap();
        match received {
            Telemetry::Event { source, kind, data, .. } => {
                assert_eq!(source, "test");
                assert_eq!(kind, "event");
                assert_eq!(data, vec![1.0, 2.0]);
            }
            _ => panic!("Expected Event telemetry"),
        }
    }

    fn test_micro_control_observer() {
        let (tel_tx, tel_rx) = crossbeam_channel::unbounded();
        
        // Исправление: создаем наблюдателя с Sender, а не очередью
        let observer = MicroControlObserver::with_sender(tel_tx); // или изменить new()

        observer.record_violation("test_servo", 100, 250, Some(0.5));

        let stats = observer.sandbox_summary();
        assert_eq!(stats.total_violations, 1);

        // Проверяем, что пришло Violation, а не Event
        let received = tel_rx.try_recv().unwrap();
        match received {
            Telemetry::Violation { component, expected_ns, actual_ns, value, .. } => {
                assert_eq!(component, "test_servo");
                assert_eq!(expected_ns, 100);
                assert_eq!(actual_ns, 250);
                assert_eq!(value, Some(0.5));
            }
            _ => panic!("Expected violation telemetry, got {:?}", received.kind()),
        }
    }

    #[test]
    fn test_micro_control_permit() {
        let permit = MicroControlPermit::new("test", 1000);

        assert!(permit.is_allowed());
        assert_eq!(permit.max_time_ns(), 1000);
        assert_eq!(permit.component(), "test");

        permit.revoke();
        assert!(!permit.is_allowed());
    }

    #[test]
    fn test_operation_guard() {
        let telemetry_queue = TelemetryQueue::new("test");
        let tel_tx = telemetry_queue.sender(); // получаем Sender для отправки
        let tel_rx = telemetry_queue.receiver(); // получаем Receiver для получения
        let observer = MicroControlObserver::new(telemetry_queue); // передаем очередь
        
        {
            let _guard = observer.observe_start("test_op");
            std::thread::sleep(std::time::Duration::from_micros(10));
        } // guard автоматически фиксирует завершение при drop

        let stats = observer.sandbox_summary();
        assert_eq!(stats.total_operations, 1);
        assert!(stats.max_time_ns > 0);
    }

    #[test]
    fn test_command_type_conversion() {
        let port = dummy_port();
        let param = dummy_param();

        let cmd = SetParameter::new(port, param, 0.5, SignalSource::Manual);
        
        let command = CommandEnum::SetParameter(cmd);
        
        match command {
            CommandEnum::SetParameter(ref sp) => {
                assert_eq!(sp.value, 0.5);
                assert!(matches!(sp.source, SignalSource::Manual));
            }
            _ => panic!("Wrong command type"),
        }

        assert_eq!(command.command_type(), CommandType::SetParameter);
    }

    #[test]
    fn test_signal_source_display() {
        let source = SignalSource::Automaton("lfo".into());
        assert_eq!(format!("{}", source), "⚙️ lfo");

        let source = SignalSource::Sensor("knob".into());
        assert_eq!(format!("{}", source), "👁️ knob");

        let source = SignalSource::Servo("filter".into());
        assert_eq!(format!("{}", source), "🦾 filter");

        let source = SignalSource::External("midi".into());
        assert_eq!(format!("{}", source), "🌍 midi");

        let source = SignalSource::Manual;
        assert_eq!(format!("{}", source), "👤 manual");

        let source = SignalSource::Script;
        assert_eq!(format!("{}", source), "📜 script");
    }

    #[test]
    fn test_queue_stats() {
        let queue: CommandQueue<SetParameter> = CommandQueue::new("test");
        
        let stats = queue.stats();
        assert_eq!(stats.name, "test");
        assert_eq!(stats.current_size, 0);
        assert_eq!(stats.max_size, 0);
        assert!(!stats.is_bounded);

        let bounded: CommandQueue<SetParameter> = CommandQueue::with_capacity("bounded", 10);
        let stats = bounded.stats();
        assert!(stats.is_bounded);
        assert_eq!(stats.capacity, Some(10));
    }

    #[test]
    fn test_telemetry_kind() {
        let port = dummy_port();
        
        let telemetry = Telemetry::peak(port, 0.8);
        assert_eq!(telemetry.kind(), TelemetryKind::Peak);

        let telemetry = Telemetry::parameter(port, dummy_param(), 0.5);
        assert_eq!(telemetry.kind(), TelemetryKind::Parameter);

        let telemetry = Telemetry::audio(NodeId(1), 0, vec![0.1, 0.2]);
        assert_eq!(telemetry.kind(), TelemetryKind::Audio);

        let telemetry = Telemetry::event("test", "test", vec![]);
        assert_eq!(telemetry.kind(), TelemetryKind::Event);
    }

    #[test]
    fn test_queue_cloning() {
        let queue: CommandQueue<SetParameter> = CommandQueue::new("test");
        let sender = queue.sender();
        let receiver = queue.receiver();

        // Можно клонировать отправители и получатели
        let sender2 = sender.clone();
        let receiver2 = receiver.clone();

        let port = dummy_port();
        let param = dummy_param();
        let cmd = SetParameter::new(port, param, 0.5, SignalSource::Manual);

        sender.send(cmd).unwrap();
        
        // Любой получатель может прочитать
        let _ = receiver2.try_recv().unwrap();
    }

    #[test]
    fn test_queue_iter() {
        let queue: CommandQueue<SetParameter> = CommandQueue::new("test");
        let port = dummy_port();
        let param = dummy_param();

        for i in 0..5 {
            let cmd = SetParameter::new(port, param.clone(), i as f32 * 0.1, SignalSource::Manual);
            queue.send(cmd).unwrap();
        }

        let mut count = 0;
        for cmd in queue.iter() {
            assert!(cmd.value >= 0.0 && cmd.value <= 0.5);
            count += 1;
        }
        assert_eq!(count, 5);
    }

    #[test]
    fn test_observer_component_stats() {
        let telemetry_queue = TelemetryQueue::new("test");
        let tel_tx = telemetry_queue.sender(); // получаем Sender для отправки
        let tel_rx = telemetry_queue.receiver(); // получаем Receiver для получения
        let observer = MicroControlObserver::new(telemetry_queue); // передаем очередь

        for i in 0..5 {
            let _guard = observer.observe_start("comp1");
            std::thread::sleep(std::time::Duration::from_micros(i * 10));
        }

        for i in 0..3 {
            let _guard = observer.observe_start("comp2");
            std::thread::sleep(std::time::Duration::from_micros(i * 20));
        }

        let stats = observer.sandbox_summary();
        assert_eq!(stats.total_operations, 8);
        assert_eq!(stats.components.len(), 2);

        let comp1_stats = observer.component_stats("comp1").unwrap();
        assert_eq!(comp1_stats.operations, 5);
        assert!(comp1_stats.avg_time_ns > 0.0);

        let comp2_stats = observer.component_stats("comp2").unwrap();
        assert_eq!(comp2_stats.operations, 3);
    }
}