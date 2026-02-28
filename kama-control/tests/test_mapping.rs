use kama_control::{
    ControlEngine, ControlEvent, EventPattern, Mapping, Target, Transform,
};
use kama_core::traits::{NodeId, ParameterId, PortId};
use crossbeam_channel::unbounded;

fn test_param(name: &str) -> ParameterId {
    ParameterId::new(name).unwrap()
}

#[test]
fn test_complete_workflow() {
    let mut engine = ControlEngine::new();
    let (tx, rx) = unbounded();
    engine.set_output_channel(tx);
    
    let node = NodeId(1);
    let port = PortId::control_in(node, 0);
    
    // Создаем несколько маппингов
    let mappings = vec![
        Mapping::new(
            EventPattern::Knob(1),
            Target::new(port, test_param("gain"), 0.0, 1.0),
            Transform::Linear,
        ),
        Mapping::new(
            EventPattern::Knob(1),
            Target::new(port, test_param("pan"), -1.0, 1.0),
            Transform::Linear,
        ),
        Mapping::new(
            EventPattern::Button(2),
            Target::new(port, test_param("mute"), 0.0, 1.0),
            Transform::Linear,
        ),
    ];
    
    engine.add_mappings(mappings);
    
    // Обрабатываем события
    engine.process_event(ControlEvent::Knob { id: 1, value: 0.3 });
    engine.process_event(ControlEvent::Button { id: 2, pressed: true });
    engine.process_event(ControlEvent::Knob { id: 1, value: 0.7 });
    
    // Проверяем статистику
    let stats = engine.stats();
    assert_eq!(stats.events_processed, 3);
    assert_eq!(stats.mappings_applied, 4); // 2 от Knob + 2 от Button? Нет, Button только один маппинг
    
    // Должно быть 3 сигнала: gain(0.3), mute(1.0), gain(0.7)
    let mut signals = Vec::new();
    while let Ok(s) = rx.try_recv() {
        signals.push(s);
    }
    
    assert_eq!(signals.len(), 3);
}

#[test]
fn test_pattern_matching() {
    let mut engine = ControlEngine::new();
    let (tx, rx) = unbounded();
    engine.set_output_channel(tx);
    
    let node = NodeId(1);
    let port = PortId::control_in(node, 0);
    
    // Специфичные маппинги
    engine.add_mapping(Mapping::new(
        EventPattern::Knob(1),
        Target::new(port, test_param("knob1"), 0.0, 1.0),
        Transform::Linear,
    ));
    
    engine.add_mapping(Mapping::new(
        EventPattern::AnyKnob,
        Target::new(port, test_param("any_knob"), 0.0, 1.0),
        Transform::Linear,
    ));
    
    engine.add_mapping(Mapping::new(
        EventPattern::Any,
        Target::new(port, test_param("catch_all"), -1.0, 1.0),
        Transform::Linear,
    ));
    
    // Событие должно подойти под 3 маппинга
    engine.process_event(ControlEvent::Knob { id: 1, value: 0.5 });
    
    let mut count = 0;
    while let Ok(_) = rx.try_recv() {
        count += 1;
    }
    
    assert_eq!(count, 3);
}

#[test]
fn test_transform_custom() {
    use std::sync::Arc;
    
    let mut engine = ControlEngine::new();
    let (tx, rx) = unbounded();
    engine.set_output_channel(tx);
    
    let node = NodeId(1);
    let port = PortId::control_in(node, 0);
    
    // Кастомное преобразование: квадратичное с ограничением
    let transform = Transform::Custom(Arc::new(|x| {
        (x * x).min(0.8)
    }));
    
    engine.add_mapping(Mapping::new(
        EventPattern::Knob(1),
        Target::new(port, test_param("custom"), 0.0, 10.0),
        transform,
    ));
    
    engine.process_event(ControlEvent::Knob { id: 1, value: 0.9 });
    
    let signal = rx.try_recv().unwrap();
    // 0.9^2 = 0.81, min(0.81, 0.8) = 0.8, потом *10 = 8.0
    assert!((signal.value - 8.0).abs() < 0.1);
}