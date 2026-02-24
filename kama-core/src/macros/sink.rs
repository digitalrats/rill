/// Макрос для создания простого приёмника (только для тестов)
#[macro_export]
macro_rules! sink_node_f32 {
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident {
            // Секция параметров (может быть пустой)
            params {
                $(
                    $(#[$field_meta:meta])*
                    $field_vis:vis $field_name:ident: f32 = $field_default:expr
                ),* $(,)?
            }
            
            // Секция состояния (обязательная)
            state {
                $state_vis:vis $state_name:ident: $state_type:ty = $state_default:expr
            }
        }
        
        ports {
            audio_in: $num_inputs:expr,
            $(control: $control:expr)?
        }
        
        process_fn: $process:expr,
        reset_fn: $reset:expr,
    ) => {
        $(#[$meta])*
        $vis struct $name {
            $($(#[$field_meta])* $field_vis $field_name: f32),*,
            $state_vis $state_name: $state_type,
            sample_rate: f32,
        }

        impl $name {
            pub fn new($($field_name: f32),*) -> Self {
                Self {
                    $($field_name),*,
                    $state_name: $state_default,
                    sample_rate: 44100.0,
                }
            }
        }

        // ... остальная реализация
    };
}