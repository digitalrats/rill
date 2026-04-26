use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum ClassicSystem {
    Nes,
    Commodore64,
    SegaGenesis,
    RolandD50,
    AkaiS900,
    EmulatorII,
    FairlightCMI,
    LinnDrum,
    Custom {
        bit_depth: u8,
        sample_rate: f32,
        nonlinear: bool,
        noise_floor: f32,
    },
}

impl ClassicSystem {
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
    
    pub fn has_nonlinear_encoding(&self) -> bool {
        matches!(self, 
            ClassicSystem::AkaiS900 | 
            ClassicSystem::Custom { nonlinear: true, .. }
        )
    }
    
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareEmulation {
    pub bit_depth: u8,
    pub sample_rate: f32,
    pub dac_nonlinearity: bool,
    pub clock_drift: f32,
    pub voltage_drop: f32,
    pub crosstalk: f32,
    pub thermal_noise: f32,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LofiConfig {
    pub system: ClassicSystem,
    pub hardware: HardwareEmulation,
    pub enable_bitcrush: bool,
    pub enable_sr_reduction: bool,
    pub enable_noise: bool,
    pub output_gain: f32,
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
    pub fn for_system(system: ClassicSystem) -> Self {
        Self {
            system,
            hardware: HardwareEmulation::for_system(system),
            ..Default::default()
        }
    }
}