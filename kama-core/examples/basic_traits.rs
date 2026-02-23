//! Базовый пример использования трейтов из kama-core

use kama_core::traits::*;

fn main() {
    println!("=== Basic Traits Example ===");
    
    // Создаём идентификатор узла
    let node_id = NodeId(42);
    println!("Node ID: {}", node_id);
    
    // Создаём параметры
    let float_param = ParamValue::Float(0.5);
    let int_param = ParamValue::Int(10);
    let bool_param = ParamValue::Bool(true);
    
    println!("Float param: {:?}", float_param);
    println!("Int param: {:?}", int_param);
    println!("Bool param: {:?}", bool_param);
    
    println!("\n✅ Traits example completed!");
}