//! Тесты совместимости - проверяем, что новый API работает

use kama_buffers::*;

#[test]
fn test_new_manager_api() {
    // Используем новый менеджер
    let manager = BufferManager::new();
    let node_id = NodeId(1);
    
    // Тест with_buffers_mut
    let result = manager.with_buffers_mut(node_id, 2, 2, 256, |buffers| {
        assert_eq!(buffers.inputs.len(), 2);
        assert_eq!(buffers.outputs.len(), 2);
        assert_eq!(buffers.inputs[0].len(), 256);
        42
    });
    
    assert_eq!(result, 42);
    
    // Статистика
    let stats = manager.stats();
    assert_eq!(stats.active_nodes, 1);
    assert_eq!(stats.active_buffers, 4);
}

#[test]
fn test_acquire_named_api() {
    let manager = BufferManager::new();
    let initial_available = manager.stats().pool_available;
    
    // Acquire and register - для долгоживущих буферов
    let buffer = manager.acquire_named("test", 256).unwrap();
    assert_eq!(buffer.read().len(), 256);
    assert_eq!(manager.stats().pool_available, initial_available - 1);
    assert_eq!(manager.stats().registered_buffers, 1);
    
    // Get by name
    let retrieved = manager.get_vector("test").unwrap();
    assert_eq!(retrieved.read().len(), 256);
    
    // Unregister (удаляем из реестра, но буфер еще жив)
    assert!(manager.unregister("test"));
    assert!(!manager.contains("test"));
    assert_eq!(manager.stats().registered_buffers, 0);
    assert_eq!(manager.stats().pool_available, initial_available - 1);
    
    // Буфер все еще существует, пул не восстановлен
    drop(buffer);
    
    // После drop пул все еще не восстановлен, потому что Arc не знает о пуле
    // Это ожидаемо для именованных буферов
    assert_eq!(manager.stats().pool_available, initial_available - 1);
}

#[test]
fn test_acquire_named_and_release() {
    let manager = BufferManager::new();
    let initial_available = manager.stats().pool_available;
    
    // Создаем именованный буфер в отдельном scope
    {
        let buffer = manager.acquire_named("test", 256).unwrap();
        assert_eq!(manager.stats().pool_available, initial_available - 1);
        assert_eq!(manager.stats().registered_buffers, 1);
    } // buffer умирает здесь
    
    // После того как buffer умер, но имя все еще в реестре
    assert_eq!(manager.stats().registered_buffers, 1);
    assert_eq!(manager.stats().pool_available, initial_available - 1);
    
    // Теперь удаляем из реестра и возвращаем в пул
    assert!(manager.unregister_and_release("test"));
    assert_eq!(manager.stats().registered_buffers, 0);
    assert_eq!(manager.stats().pool_available, initial_available);
}

#[test]
fn test_acquire_pooled_auto_release() {
    let manager = BufferManager::new();
    let initial_available = manager.stats().pool_available;
    
    // Используем acquire_pooled для временных буферов
    {
        let buffer = manager.acquire_pooled(256).unwrap();
        assert_eq!(buffer.len(), 256);
        assert_eq!(manager.stats().pool_available, initial_available - 1);
        
        // Можно использовать как slice
        let slice = buffer.as_slice();
        assert_eq!(slice.len(), 256);
    } // buffer автоматически возвращается в пул при drop
    
    assert_eq!(manager.stats().pool_available, initial_available);
}

#[test]
fn test_acquire_release_api() {
    let manager = BufferManager::new();
    let initial_available = manager.stats().pool_available;
    
    // Acquire pooled buffer
    let buffer = manager.acquire_pooled(256).unwrap();
    assert_eq!(buffer.len(), 256);
    assert_eq!(manager.stats().pool_available, initial_available - 1);
    
    // Release происходит автоматически при drop
    drop(buffer);
    assert_eq!(manager.stats().pool_available, initial_available);
}

#[test]
fn test_ring_buffer_compatibility() {
    // Проверяем, что RingBuffer не изменился
    let mut ring = RingBuffer::new(8);
    ring.write(&[1.0, 2.0, 3.0, 4.0]);
    
    let mut output = vec![0.0; 4];
    ring.read(1, &mut output);
    assert_eq!(output, [4.0, 3.0, 2.0, 1.0]);
}

#[test]
fn test_multi_head_compatibility() {
    // Проверяем, что MultiHeadBuffer не изменился
    let mut buffer = MultiHeadBuffer::new(1024, 44100.0);
    let head_id = buffer.add_head();
    
    assert!(buffer.get_head(head_id).is_some());
    
    let test_data = vec![0.5; 256];
    buffer.write(&test_data);
    
    let mut output_left = vec![0.0; 64];
    let mut output_right = vec![0.0; 64];
    let mut outputs = [&mut output_left[..], &mut output_right[..]];
    
    buffer.process(&[], &mut outputs).unwrap();
    
    assert!(output_left.iter().any(|&x| x != 0.0) || output_right.iter().any(|&x| x != 0.0));
}

#[test]
fn test_pooled_buffer_auto_release() {
    let manager = BufferManager::new();
    let initial_available = manager.stats().pool_available;
    
    {
        let buffer = manager.acquire(256).unwrap();
        assert_eq!(buffer.len(), 256);
        assert_eq!(manager.stats().pool_available, initial_available - 1);
        
        // Можно использовать как slice
        let slice = buffer.as_slice();
        assert_eq!(slice.len(), 256);
    } // Здесь buffer автоматически возвращается в пул
    
    assert_eq!(manager.stats().pool_available, initial_available);
}

#[test]
fn test_create_ring() {
    let manager = BufferManager::new();
    
    let ring = manager.create_ring("test_ring", 1024);
    assert_eq!(ring.read().size(), 1024);
    
    assert!(manager.contains("test_ring"));
    
    let retrieved = manager.get_ring("test_ring").unwrap();
    assert_eq!(retrieved.read().size(), 1024);
}

#[test]
fn test_stats() {
    let manager = BufferManager::new();
    let initial_available = manager.stats().pool_available;
    
    let _buf1 = manager.acquire_named("buf1", 256).unwrap();
    let _buf2 = manager.acquire_named("buf2", 256).unwrap();
    
    let stats = manager.stats();
    assert_eq!(stats.registered_buffers, 2);
    assert_eq!(stats.pool_available, initial_available - 2);
}

#[test]
fn test_clear_all() {
    let manager = BufferManager::new();
    let initial_available = manager.stats().pool_available;
    
    let _buf1 = manager.acquire_named("buf1", 256).unwrap();
    let _buf2 = manager.acquire_named("buf2", 256).unwrap();
    
    let node_id = NodeId(1);
    manager.with_buffers_mut(node_id, 1, 1, 128, |_| {});
    
    assert_eq!(manager.stats().registered_buffers, 2);
    assert_eq!(manager.stats().active_nodes, 1);
    
    manager.clear_all();
    
    assert_eq!(manager.stats().registered_buffers, 0);
    assert_eq!(manager.stats().active_nodes, 0);
    assert_eq!(manager.stats().pool_available, initial_available);
}