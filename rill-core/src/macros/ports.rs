//! # Вспомогательные макросы для работы с портами

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
        {
            let mut node = $node;
            $(
                // Создаем ParameterId из строки
                let param_id = match $crate::ParameterId::new(stringify!($name)) {
                    Ok(id) => id,
                    Err(e) => panic!("Invalid parameter name '{}': {:?}", stringify!($name), e),
                };
                // Преобразуем значение в ParamValue
                let param_value: $crate::ParamValue = $value.into();
                match node.set_parameter(&param_id, param_value) {
                    Ok(()) => {},
                    Err(e) => panic!("Failed to set parameter '{}': {:?}", stringify!($name), e),
                }
            )*
            node
        }
    };
}
