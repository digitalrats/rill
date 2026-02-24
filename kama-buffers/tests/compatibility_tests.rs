//! Тесты совместимости - проверяем, что новый API работает

use kama_buffers::*;

#[test]
fn test_new_manager_api() {
    // Используем новый менеджер
    let manager = BufferManager::new();

    // Статистика
    let stats = manager.stats();
    assert_eq!(stats.active_buffers, 0);
}

#[test]
fn test_acquire_release_api() {
    let manager = BufferManager::new();
    let initial_available = manager.stats().pool_available;

    // Acquire буфера
    let buffer = manager.acquire(256).unwrap();
    assert_eq!(buffer.len(), 256);
    assert_eq!(manager.stats().pool_available, initial_available - 1);

    // Release происходит автоматически при drop
    drop(buffer);
    assert_eq!(manager.stats().pool_available, initial_available);
}

#[test]
fn test_acquire_named_api() {
    let manager = BufferManager::new();
    let initial_available = manager.stats().pool_available;

    // Acquire and register
    let buffer = manager.acquire_named("test", 256).unwrap();
    assert_eq!(buffer.read().len(), 256);
    assert_eq!(manager.stats().pool_available, initial_available - 1);
    assert_eq!(manager.stats().registered_buffers, 1);

    // Get by name
    let retrieved = manager.get_vector("test").unwrap();
    assert_eq!(retrieved.read().len(), 256);

    // Unregister (буфер еще жив, потому что у нас есть buffer)
    assert!(manager.unregister("test"));
    assert!(!manager.contains("test"));
    assert_eq!(manager.stats().registered_buffers, 0);

    // После unregister пул все еще уменьшен на 1, потому что buffer еще жив
    assert_eq!(manager.stats().pool_available, initial_available - 1);

    // Release buffer
    drop(buffer);

    // После drop память освобождается, но в пул не возвращается
    // Это нормальное поведение для именованных буферов
    assert_eq!(manager.stats().pool_available, initial_available - 1);
}

#[test]
fn test_acquire_named_and_release() {
    let manager = BufferManager::new();
    let initial_available = manager.stats().pool_available;

    // Создаем именованный буфер
    {
        let buffer = manager.acquire_named("test", 256).unwrap();
        assert_eq!(manager.stats().pool_available, initial_available - 1);
        assert_eq!(manager.stats().registered_buffers, 1);
    } // buffer умирает здесь

    assert_eq!(manager.stats().registered_buffers, 1); // имя все еще в реестре
    assert_eq!(manager.stats().pool_available, initial_available - 1);

    // Удаляем из реестра и возвращаем в пул
    assert!(manager.unregister_and_release("test"));
    assert_eq!(manager.stats().registered_buffers, 0);
    assert_eq!(manager.stats().pool_available, initial_available);
}

#[test]
fn test_acquire_pooled_auto_release() {
    let manager = BufferManager::new();
    let initial_available = manager.stats().pool_available;

    // Используем acquire для временных буферов
    {
        let buffer = manager.acquire(256).unwrap();
        assert_eq!(buffer.len(), 256);
        assert_eq!(manager.stats().pool_available, initial_available - 1);

        assert_eq!(buffer.len(), 256);
        assert_eq!(manager.stats().pool_available, initial_available - 1);

        // Можно использовать как slice
        let slice = buffer.as_slice();
        assert_eq!(slice.len(), 256);
    } // buffer автоматически возвращается в пул при drop

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

    assert_eq!(manager.stats().registered_buffers, 2);

    manager.clear_all();

    assert_eq!(manager.stats().registered_buffers, 0);
    assert_eq!(manager.stats().pool_available, initial_available);
}
