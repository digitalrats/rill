//! # Вспомогательные макросы для работы с параметрами

/// Разбор параметров из синтаксиса макроса
#[macro_export]
#[doc(hidden)]
macro_rules! __parse_params {
    () => {};
    ({ $($name:ident: $ty:ty = $default:expr),* $(,)? }) => {
        $(pub $name: $ty),*
    };
    (params { $($name:ident: $ty:ty = $default:expr),* $(,)? }) => {
        $(pub $name: $ty),*
    };
}

/// Установка значений параметров по умолчанию
#[macro_export]
#[doc(hidden)]
macro_rules! __parse_params_defaults {
    () => {};
    ({ $($name:ident: $ty:ty = $default:expr),* $(,)? }) => {
        $($name: $default),*
    };
    (params { $($name:ident: $ty:ty = $default:expr),* $(,)? }) => {
        $($name: $default),*
    };
}

/// Установка параметра по имени
#[macro_export]
#[doc(hidden)]
macro_rules! __set_param {
    ({ $($name:ident: $ty:ty = $default:expr),* $(,)? }, $self:expr, $key:expr, $value:expr) => {
        match $key {
            $(stringify!($name) => {
                $self.$name = $value;
                Ok(())
            })*
            _ => Err($crate::error::Error::new(
                $crate::error::ErrorCode::InvalidParameter,
                format!("Unknown parameter: {}", $key)
            )),
        }
    };
    (params { $($name:ident: $ty:ty = $default:expr),* $(,)? }, $self:expr, $key:expr, $value:expr) => {
        match $key {
            $(stringify!($name) => {
                $self.$name = $value;
                Ok(())
            })*
            _ => Err($crate::error::Error::new(
                $crate::error::ErrorCode::InvalidParameter,
                format!("Unknown parameter: {}", $key)
            )),
        }
    };
    () => {
        Err($crate::error::Error::new(
            $crate::error::ErrorCode::InvalidParameter,
            "No parameters defined".to_string()
        ))
    };
}

/// Получение параметра по имени
#[macro_export]
#[doc(hidden)]
macro_rules! __get_param {
    ({ $($name:ident: $ty:ty = $default:expr),* $(,)? }, $self:expr, $key:expr) => {
        match $key {
            $(stringify!($name) => Some($self.$name),)*
            _ => None,
        }
    };
    (params { $($name:ident: $ty:ty = $default:expr),* $(,)? }, $self:expr, $key:expr) => {
        match $key {
            $(stringify!($name) => Some($self.$name),)*
            _ => None,
        }
    };
    () => { None };
}

/// Получение списка имён параметров
#[macro_export]
#[doc(hidden)]
macro_rules! __param_names {
    ({ $($name:ident: $ty:ty = $default:expr),* $(,)? }) => {
        vec![$(stringify!($name)),*]
    };
    (params { $($name:ident: $ty:ty = $default:expr),* $(,)? }) => {
        vec![$(stringify!($name)),*]
    };
    () => { Vec::new() };
}