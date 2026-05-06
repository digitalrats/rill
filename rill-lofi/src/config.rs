use serde::{Deserialize, Serialize};

/// Classic digital audio systems that inform the lo-fi emulation parameters.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum ClassicSystem {
    /// Nintendo Entertainment System (7-bit).
    Nes,
    /// Commodore 64 (8-bit).
    Commodore64,
    /// Sega Genesis / Mega Drive (9-bit).
    SegaGenesis,
    /// Roland D-50 (16-bit, 32 kHz).
    RolandD50,
    /// Akai S900 (12-bit, 40 kHz, non-linear).
    AkaiS900,
    /// E-mu Emulator II (8-bit, 27.7 kHz).
    EmulatorII,
    /// Fairlight CMI (8-bit, 16 kHz).
    FairlightCMI,
    /// LinnDrum (8-bit).
    LinnDrum,
    /// User-defined system with custom parameters.
    Custom {
        /// Bit depth (1–16).
        bit_depth: u8,
        /// Sample rate in Hz.
        sample_rate: f32,
        /// Whether the system uses non-linear encoding.
        nonlinear: bool,
        /// Noise floor in dB.
        noise_floor: f32,
    },
}

impl ClassicSystem {
    /// Returns the bit depth for this classic system.
    pub fn get_bit_depth(&self) -> u8 {
        match self {
            ClassicSystem::Nes => 7,
            ClassicSystem::Commodore64 => 8,
            ClassicSystem::SegaGenesis => 9,
            ClassicSystem::RolandD50 => 16,
            ClassicSystem::AkaiS900 => 12,
            ClassicSystem::EmulatorII => 8,
            ClassicSystem::FairlightCMI => 8,
            ClassicSystem::LinnDrum => 8,
            ClassicSystem::Custom { bit_depth, .. } => *bit_depth,
        }
    }

    /// Returns the sample rate in Hz for this classic system.
    pub fn get_sample_rate(&self) -> f32 {
        match self {
            ClassicSystem::Nes => 44_100.0,
            ClassicSystem::Commodore64 => 44_100.0,
            ClassicSystem::SegaGenesis => 44_100.0,
            ClassicSystem::RolandD50 => 32_000.0,
            ClassicSystem::AkaiS900 => 40_000.0,
            ClassicSystem::EmulatorII => 27_700.0,
            ClassicSystem::FairlightCMI => 16_000.0,
            ClassicSystem::LinnDrum => 44_100.0,
            ClassicSystem::Custom { sample_rate, .. } => *sample_rate,
        }
    }

    /// Returns `true` if this system uses non-linear encoding (e.g. Akai S900).
    pub fn has_nonlinear_encoding(&self) -> bool {
        matches!(
            self,
            ClassicSystem::AkaiS900
                | ClassicSystem::Custom {
                    nonlinear: true,
                    ..
                }
        )
    }

    /// Returns the noise floor in dB for this classic system.
    pub fn get_noise_floor_db(&self) -> f32 {
        match self {
            ClassicSystem::Nes => -42.0,
            ClassicSystem::Commodore64 => -48.0,
            ClassicSystem::AkaiS900 => -72.0,
            ClassicSystem::FairlightCMI => -48.0,
            ClassicSystem::Custom { noise_floor, .. } => *noise_floor,
            _ => -90.0,
        }
    }
}

/// Parameters for emulating vintage hardware imperfections.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareEmulation {
    /// Bit depth of the DAC.
    pub bit_depth: u8,
    /// Sample rate in Hz.
    pub sample_rate: f32,
    /// Whether to simulate DAC non-linearity.
    pub dac_nonlinearity: bool,
    /// Clock drift factor.
    pub clock_drift: f32,
    /// Voltage drop factor.
    pub voltage_drop: f32,
    /// Channel crosstalk factor.
    pub crosstalk: f32,
    /// Thermal noise floor.
    pub thermal_noise: f32,
    /// Ageing effect factor.
    pub ageing_effect: f32,
}

impl Default for HardwareEmulation {
    fn default() -> Self {
        Self {
            bit_depth: 8,
            sample_rate: 44_100.0,
            dac_nonlinearity: true,
            clock_drift: 0.1,
            voltage_drop: 0.02,
            crosstalk: 0.01,
            thermal_noise: 0.001,
            ageing_effect: 0.05,
        }
    }
}

impl HardwareEmulation {
    /// Creates a `HardwareEmulation` configured for a given classic system.
    pub fn for_system(system: ClassicSystem) -> Self {
        let mut emulation = Self::default();

        match system {
            ClassicSystem::Nes => {
                emulation.bit_depth = 7;
                emulation.clock_drift = 0.5;
                emulation.voltage_drop = 0.05;
                emulation.thermal_noise = 0.005;
            }
            ClassicSystem::Commodore64 => {
                emulation.bit_depth = 8;
                emulation.dac_nonlinearity = true;
                emulation.clock_drift = 0.3;
                emulation.crosstalk = 0.03;
            }
            ClassicSystem::AkaiS900 => {
                emulation.bit_depth = 12;
                emulation.dac_nonlinearity = true;
                emulation.sample_rate = 40_000.0;
                emulation.thermal_noise = 0.001;
            }
            ClassicSystem::FairlightCMI => {
                emulation.bit_depth = 8;
                emulation.sample_rate = 16_000.0;
                emulation.clock_drift = 1.0;
                emulation.voltage_drop = 0.1;
                emulation.thermal_noise = 0.01;
            }
            _ => {
                emulation.bit_depth = system.get_bit_depth();
                emulation.sample_rate = system.get_sample_rate();
            }
        }

        emulation
    }
}

/// Configuration for the lo-fi audio processor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LofiConfig {
    /// Target classic system to emulate.
    pub system: ClassicSystem,
    /// Hardware imperfection parameters.
    pub hardware: HardwareEmulation,
    /// Enable bitcrushing effect.
    pub enable_bitcrush: bool,
    /// Enable sample rate reduction.
    pub enable_sr_reduction: bool,
    /// Enable noise simulation.
    pub enable_noise: bool,
    /// Output gain (1.0 = unity).
    pub output_gain: f32,
    /// Dry/wet mix (1.0 = fully wet).
    pub dry_wet: f32,
}

impl Default for LofiConfig {
    fn default() -> Self {
        Self {
            system: ClassicSystem::Custom {
                bit_depth: 8,
                sample_rate: 44_100.0,
                nonlinear: false,
                noise_floor: -48.0,
            },
            hardware: HardwareEmulation::default(),
            enable_bitcrush: true,
            enable_sr_reduction: true,
            enable_noise: true,
            output_gain: 1.0,
            dry_wet: 1.0,
        }
    }
}

impl LofiConfig {
    /// Creates a `LofiConfig` pre-populated for a given classic system.
    pub fn for_system(system: ClassicSystem) -> Self {
        Self {
            system,
            hardware: HardwareEmulation::for_system(system),
            ..Default::default()
        }
    }
}
