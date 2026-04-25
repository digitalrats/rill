//! Трейты для эффектов

use crate::algorithm::{Algorithm, ParameterizedAlgorithm};
use rill_core::AudioNum;

/// Базовый трейт для эффектов
pub trait Effect<T: AudioNum>: ParameterizedAlgorithm<T> {
    /// Получить количество входных каналов
    fn num_inputs(&self) -> usize {
        1
    }

    /// Получить количество выходных каналов
    fn num_outputs(&self) -> usize {
        1
    }

    /// Обработать стерео пару (если поддерживается)
    fn process_stereo(&mut self, left: T, right: T) -> (T, T) {
        let input = [left, right];
        let mut output = [T::ZERO, T::ZERO];
        self.process_block(&input, &mut output);
        (output[0], output[1])
    }

    /// Обработать блок с использованием векторного eDSL (опционально)
    fn process_block_vector(&mut self, input: &[T], output: &mut [T]) {
        self.process_block(input, output);
    }
}

/// Эффект с возможностью bypass
pub trait Bypassable<T: AudioNum>: Effect<T> {
    /// Включить/выключить bypass
    fn set_bypass(&mut self, bypass: bool);

    /// Текущее состояние bypass
    fn bypass(&self) -> bool;

    /// Обработка с учётом bypass
    fn process_with_bypass(&mut self, input: T) -> T {
        if self.bypass() {
            input
        } else {
            let mut output = [T::ZERO];
            self.process_block(&[input], &mut output);
            output[0]
        }
    }
}

/// Эффект с поддержкой dry/wet
pub trait DryWet<T: AudioNum>: Effect<T> {
    /// Установить соотношение dry/wet (0.0 = только dry, 1.0 = только wet)
    fn set_dry_wet(&mut self, mix: f32);

    /// Текущее значение dry/wet
    fn dry_wet(&self) -> f32;

    /// Обработка с учётом dry/wet
    fn process_with_dry_wet(&mut self, input: T, dry: T) -> T {
        let mut wet = [T::ZERO];
        self.process_block(&[input], &mut wet);
        let mix = T::from_f32(self.dry_wet());
        let one_minus_mix = T::from_f32(1.0 - self.dry_wet());

        dry.mul(one_minus_mix).add(wet[0].mul(mix))
    }
}

/// Эффект с модуляцией
pub trait Modulatable<T: AudioNum>: Effect<T> {
    /// Количество модуляционных входов
    fn num_mod_inputs(&self) -> usize;

    /// Применить модуляцию
    fn apply_modulation(&mut self, index: usize, value: T);

    /// Глубина модуляции
    fn modulation_depth(&self, index: usize) -> f32;

    /// Установить глубину модуляции
    fn set_modulation_depth(&mut self, index: usize, depth: f32);
}
