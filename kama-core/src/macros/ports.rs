//! # Вспомогательные макросы для работы с портами

/// Инициализация портов
#[macro_export]
#[doc(hidden)]
macro_rules! __init_ports {
    // Для Source (только выходы)
    (ports { audio_out: $out:expr $(,)? }, $node:expr, $outputs:ident) => {
        for i in 0..$out {
            let port = $crate::port::Port::output(
                $node.id,
                i as u16,
                &format!("out_{}", i)
            );
            $node.$outputs.push(port);
        }
    };
    
    // Для Sink (только входы)
    (ports { audio_in: $in:expr $(,)? }, $node:expr, $inputs:ident) => {
        for i in 0..$in {
            let port = $crate::port::Port::input(
                $node.id,
                i as u16,
                &format!("in_{}", i)
            );
            $node.$inputs.push(port);
        }
    };
    
    // Для Processor (входы и выходы)
    (ports { audio_in: $in:expr, audio_out: $out:expr $(,)? }, $node:expr, $inputs:ident, $outputs:ident, $controls:ident) => {
        for i in 0..$in {
            let port = $crate::port::Port::input(
                $node.id,
                i as u16,
                &format!("in_{}", i)
            );
            $node.$inputs.push(port);
        }
        
        for i in 0..$out {
            let port = $crate::port::Port::output(
                $node.id,
                i as u16,
                &format!("out_{}", i)
            );
            $node.$outputs.push(port);
        }
    };
    
    // С управляющими портами
    (ports { audio_in: $in:expr, audio_out: $out:expr, control: $ctrl:expr }, $node:expr, $inputs:ident, $outputs:ident, $controls:ident) => {
        $crate::__init_ports!(
            ports { audio_in: $in, audio_out: $out },
            $node,
            $inputs,
            $outputs,
            $controls
        );
        
        for i in 0..$ctrl {
            let port = $crate::port::Port::control_in(
                $node.id,
                i as u16,
                &format!("ctrl_{}", i)
            );
            $node.$controls.push(port);
        }
    };
}

/// Общий макрос для создания любого типа узла
#[macro_export]
macro_rules! audio_node {
    (
        $(#[$meta:meta])*
        source $name:ident<$T:ident: $crate::math::AudioNum, const $BUF:ident: usize>
        $(where $($bounds:tt)*)?
        { $($tt:tt)* }
    ) => {
        $crate::source_node! {
            $(#[$meta])*
            pub $name<$T, $BUF>
            $(where $($bounds)*)?
            { $($tt)* }
        }
    };
    
    (
        $(#[$meta:meta])*
        processor $name:ident<$T:ident: $crate::math::AudioNum, const $BUF:ident: usize>
        $(where $($bounds:tt)*)?
        { $($tt:tt)* }
    ) => {
        $crate::processor_node! {
            $(#[$meta])*
            pub $name<$T, $BUF>
            $(where $($bounds)*)?
            { $($tt)* }
        }
    };
    
    (
        $(#[$meta:meta])*
        sink $name:ident<$T:ident: $crate::math::AudioNum, const $BUF:ident: usize>
        $(where $($bounds:tt)*)?
        { $($tt:tt)* }
    ) => {
        $crate::sink_node! {
            $(#[$meta])*
            pub $name<$T, $BUF>
            $(where $($bounds)*)?
            { $($tt)* }
        }
    };
}

/// Добавление параметров к существующему узлу
#[macro_export]
macro_rules! with_parameters {
    (
        $node:expr,
        $($name:ident: $value:expr),* $(,)?
    ) => {
        $(
            $node.set_parameter(stringify!($name), $value)?;
        )*
        $node
    };
}

/// Добавление портов к существующему узлу
#[macro_export]
macro_rules! with_ports {
    (
        $node:expr,
        $($port_type:ident: $count:expr),* $(,)?
    ) => {
        {
            let mut node = $node;
            $(
                for i in 0..$count {
                    let port = $crate::port::Port::$port_type(
                        node.id(),
                        i as u16,
                        &format!("{}_{}", stringify!($port_type), i)
                    );
                    match stringify!($port_type) {
                        "input" => node.inputs.push(port),
                        "output" => node.outputs.push(port),
                        "control" => node.controls.push(port),
                        _ => {}
                    }
                }
            )*
            node
        }
    };
}