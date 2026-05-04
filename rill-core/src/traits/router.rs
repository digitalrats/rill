//! # Router Trait — сигнальная маршрутизация
//!
//! `Router` — семантически отдельный тип графовых узлов, предназначенных
//! исключительно для маршрутизации сигналов (микшеры, матричные коммутаторы,
//! селекторы). В отличие от `Processor`, который выполняет DSP-преобразование
//! сигнала, `Router` только перераспределяет входные сигналы по выходам
//! с возможностью динамического изменения топологии соединений.
//!
//! ## Различия с Processor
//!
//! | Характеристика | Processor | Router |
//! |---|---|---|
//! | Количество I/O | Фиксированное | Динамическое N→M |
//! | DSP | Есть (фильтр, эффект) | Нет (только сумма/коммутация) |
//! | Топология | Известна на этапе сборки | Может меняться runtime |
//! | Визуализация | Прямоугольник (Р-схема) | Ромб (Р-схема, условие) |

use crate::math::Transcendental;
use crate::time::ClockTick;
use crate::traits::node::SignalNode;
use crate::traits::ProcessResult;

/// Маршрутизатор сигналов — N входов, M выходов, конфигурируемая матрица.
///
/// В отличие от `Processor::process()`, который выполняет DSP, `Router`
/// только перераспределяет входные сигналы по выходам. Маршрутизатор
/// сам управляет своими выходными портами через `SignalNode::output_port_mut()`.
///
/// `TapeLoop` получается не через этот трейт, а через реестр ресурсов графа
/// — см. `GraphBuilder::add_resource()` и `SignalNode::init()`.
pub trait Router<T: Transcendental, const BUF_SIZE: usize>: SignalNode<T, BUF_SIZE> {
    /// Выполнить маршрутизацию одного блока.
    ///
    /// Реализация должна прочитать сигналы из `inputs` и записать
    /// результаты в свои выходные порты (через `self.output_port_mut(i)`).
    fn route(&mut self, clock: &ClockTick, inputs: &[&[T; BUF_SIZE]]) -> ProcessResult<()>;

    /// Количество входных портов для маршрутизации.
    fn num_route_inputs(&self) -> usize;

    /// Количество выходных портов для маршрутизации.
    fn num_route_outputs(&self) -> usize;

    /// Установить соединение: направить вход `from` в выход `to` с коэффициентом `gain`.
    fn set_connection(&mut self, from: usize, to: usize, gain: T) -> ProcessResult<()>;

    /// Удалить соединение (обнулить коэффициент).
    fn remove_connection(&mut self, from: usize, to: usize) -> ProcessResult<()>;

    /// Получить текущую матрицу маршрутизации: для каждого выхода — список входов с гейнами.
    fn routing_matrix(&self) -> Vec<Vec<(usize, T)>>;
}
