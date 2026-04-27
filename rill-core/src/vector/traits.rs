//! # Трейты для векторных операций
//!
//! Определяет основные абстракции для работы с векторами в DSP.

use crate::AudioNum;
use std::ops::{Add, Sub, Mul, Div, Rem, Neg};

/// Основной трейт для векторных типов
pub trait Vector<T: AudioNum, const N: usize>:
    Copy + Clone + Send + Sync + 'static + Default +
    PartialEq + fmt::Debug +
    Add<Output = Self> + Sub<Output = Self> +
    Mul<Output = Self> + Div<Output = Self> +
    Rem<Output = Self> + Neg<Output = Self>
{
    /// Создает вектор, заполненный одним значением
    fn splat(value: T) -> Self;
    
    /// Загружает вектор из слайса (первые N элементов)
    fn load(slice: &[T]) -> Self;
    
    /// Сохраняет вектор в слайс (первые N элементов)
    fn store(&self, slice: &mut [T]);
    
    /// Возвращает элемент по индексу
    fn extract(&self, index: usize) -> T;
    
    /// Вставляет значение в элемент по индексу
    fn insert(&self, index: usize, value: T) -> Self;
    
    /// Поэлементное сложение
    fn add(&self, other: &Self) -> Self;
    
    /// Поэлементное вычитание
    fn sub(&self, other: &Self) -> Self;
    
    /// Поэлементное умножение
    fn mul(&self, other: &Self) -> Self;
    
    /// Поэлементное деление
    fn div(&self, other: &Self) -> Self;
    
    /// Поэлементный остаток
    fn rem(&self, other: &Self) -> Self;
    
    /// Поэлементное отрицание
    fn neg(&self) -> Self;
    
    /// Поэлементное абсолютное значение
    fn abs(&self) -> Self;
    
    /// Поэлементный минимум
    fn min(&self, other: &Self) -> Self;
    
    /// Поэлементный максимум
    fn max(&self, other: &Self) -> Self;
    
    /// Поэлементное ограничение
    fn clamp(&self, min: &Self, max: &Self) -> Self;
    
    /// Поэлементный квадратный корень
    fn sqrt(&self) -> Self;
    
    /// Поэлементная экспонента
    fn exp(&self) -> Self;
    
    /// Поэлементный натуральный логарифм
    fn ln(&self) -> Self;
    
    /// Поэлементный синус
    fn sin(&self) -> Self;
    
    /// Поэлементный косинус
    fn cos(&self) -> Self;
    
    /// Поэлементный тангенс
    fn tan(&self) -> Self;
}

/// Трейт для векторных операций со скалярами
pub trait VectorScalarOps<T: AudioNum, const N: usize> {
    /// Сложение вектора со скаляром (скаляр расширяется до вектора)
    fn add_scalar(&self, scalar: T) -> Self;
    
    /// Вычитание скаляра из вектора
    fn sub_scalar(&self, scalar: T) -> Self;
    
    /// Умножение вектора на скаляр
    fn mul_scalar(&self, scalar: T) -> Self;
    
    /// Деление вектора на скаляр
    fn div_scalar(&self, scalar: T) -> Self;
    
    /// Остаток от деления вектора на скаляр
    fn rem_scalar(&self, scalar: T) -> Self;
}

/// Трейт для редукционных операций
pub trait VectorReduce<T: AudioNum, const N: usize> {
    /// Сумма всех элементов вектора
    fn horizontal_sum(&self) -> T;
    
    /// Произведение всех элементов вектора
    fn horizontal_product(&self) -> T;
    
    /// Минимальный элемент вектора
    fn horizontal_min(&self) -> T;
    
    /// Максимальный элемент вектора
    fn horizontal_max(&self) -> T;
    
    /// Среднее значение элементов вектора
    fn horizontal_mean(&self) -> T;
}

/// Трейт для побитовых операций (маски и сравнения)
pub trait VectorMask<T: AudioNum, const N: usize> {
    /// Тип маски для сравнения (обычно вектор булевых значений или битовая маска)
    type Mask;
    
    /// Сравнение на равенство
    fn eq(&self, other: &Self) -> Self::Mask;
    
    /// Сравнение на неравенство
    fn ne(&self, other: &Self) -> Self::Mask;
    
    /// Больше
    fn gt(&self, other: &Self) -> Self::Mask;
    
    /// Больше или равно
    fn ge(&self, other: &Self) -> Self::Mask;
    
    /// Меньше
    fn lt(&self, other: &Self) -> Self::Mask;
    
    /// Меньше или равно
    fn le(&self, other: &Self) -> Self::Mask;
    
    /// Выбор элементов по маске
    fn select(&self, other: &Self, mask: Self::Mask) -> Self;

    /// Проверка, что все элементы маски истинны.
    ///
    /// Возвращает `true` только если каждый элемент маски указывает на истинное
    /// значение (например, все биты установлены для битовой маски, или все
    /// знаковые биты установлены для NaN-маски).
    fn all(mask: &Self::Mask) -> bool;
}

use std::fmt;
