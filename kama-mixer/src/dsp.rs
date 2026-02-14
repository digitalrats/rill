/// Тип процессора для обработки аудио
pub type AudioProcessor = Box<dyn Fn(f64) -> f64 + Send + Sync>;
/// Тип стерео процессора
pub type StereoProcessor = Box<dyn Fn(f64, f64) -> (f64, f64) + Send + Sync>;

/// Суммирование стерео сигналов
pub fn sum_stereo(signals: &[(f64, f64)]) -> (f64, f64) {
    signals.iter()
        .fold((0.0, 0.0), |(l_acc, r_acc), &(l, r)| {
            (l_acc + l, r_acc + r)
        })
}

/// Простой gain процессор
pub fn gain_processor(gain: f64) -> AudioProcessor {
    Box::new(move |input| input * gain)
}

/// Панорамирование
pub fn pan_processor(pan: f64) -> StereoProcessor {
    Box::new(move |left, right| {
        let pan = pan.clamp(-1.0, 1.0);
        let left_gain = if pan <= 0.0 { 1.0 } else { 1.0 - pan };
        let right_gain = if pan >= 0.0 { 1.0 } else { 1.0 + pan };
        
        (left * left_gain, right * right_gain)
    })
}

/// Композиция процессоров
pub fn compose_processors(
    proc1: AudioProcessor,
    proc2: AudioProcessor,
) -> AudioProcessor {
    Box::new(move |input| proc2(proc1(input)))
}

/// Композиция стерео процессоров
pub fn compose_stereo_processors(
    proc1: StereoProcessor,
    proc2: StereoProcessor,
) -> StereoProcessor {
    Box::new(move |left, right| {
        let (l, r) = proc1(left, right);
        proc2(l, r)
    })
}

/// Создаёт простой фильтр нижних частот
pub fn lowpass_processor(cutoff: f64, sample_rate: f64) -> AudioProcessor {
    let rc = 1.0 / (2.0 * std::f64::consts::PI * cutoff);
    let dt = 1.0 / sample_rate;
    let alpha = dt / (rc + dt);
    
    let mut last_output = 0.0;
    
    Box::new(move |input| {
        last_output = last_output + alpha * (input - last_output);
        last_output
    })
}