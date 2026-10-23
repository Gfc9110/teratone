//! Owned oscillator parameters and versioned preset snapshots. No DSP or shared mutable presets.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Waveform {
    Sine,
    Saw,
    Square,
    Triangle,
}
impl Waveform {
    pub fn index(self) -> u32 {
        match self {
            Self::Sine => 0,
            Self::Saw => 1,
            Self::Square => 2,
            Self::Triangle => 3,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Sine => "Sine",
            Self::Saw => "Saw",
            Self::Square => "Square / PWM",
            Self::Triangle => "Triangle",
        }
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SynthParameters {
    pub waveform: Waveform,
    pub shape: f32,
    pub pulse_width: f32,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
}
impl Default for SynthParameters {
    fn default() -> Self {
        Self {
            waveform: Waveform::Sine,
            shape: 1.0,
            pulse_width: 0.5,
            attack: 0.006,
            decay: 0.06,
            sustain: 0.75,
            release: 0.06,
        }
    }
}
impl SynthParameters {
    pub fn validate(&self) -> Result<(), String> {
        for (v, lo, hi) in [
            (self.shape, 0.0, 1.0),
            (self.pulse_width, 0.05, 0.95),
            (self.attack, 0.0, 10.0),
            (self.decay, 0.0, 10.0),
            (self.sustain, 0.0, 1.0),
            (self.release, 0.0, 20.0),
        ] {
            if !v.is_finite() || v < lo || v > hi {
                return Err("Synth parameter outside its finite range".into());
            }
        }
        Ok(())
    }
    pub fn change(&mut self, field: SynthField, value: f32) -> Result<(), String> {
        if !value.is_finite() {
            return Err("Synth parameter must be finite".into());
        }
        match field {
            SynthField::Waveform => {
                self.waveform = match value {
                    0.0 => Waveform::Sine,
                    1.0 => Waveform::Saw,
                    2.0 => Waveform::Square,
                    3.0 => Waveform::Triangle,
                    _ => return Err("Invalid waveform".into()),
                }
            }
            SynthField::Shape => self.shape = value,
            SynthField::PulseWidth => self.pulse_width = value,
            SynthField::Attack => self.attack = value,
            SynthField::Decay => self.decay = value,
            SynthField::Sustain => self.sustain = value,
            SynthField::Release => self.release = value,
        }
        self.validate()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SynthField {
    Waveform,
    Shape,
    PulseWidth,
    Attack,
    Decay,
    Sustain,
    Release,
}
impl SynthField {
    pub fn from_index(i: i32) -> Result<Self, String> {
        match i {
            0 => Ok(Self::Waveform),
            1 => Ok(Self::Shape),
            2 => Ok(Self::PulseWidth),
            3 => Ok(Self::Attack),
            4 => Ok(Self::Decay),
            5 => Ok(Self::Sustain),
            6 => Ok(Self::Release),
            _ => Err("Unknown synth parameter".into()),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SynthPreset {
    pub version: u32,
    pub name: String,
    pub source: String,
    pub parameters: SynthParameters,
}
impl SynthPreset {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self.name.trim().is_empty()
            || self.name.len() > 120
            || self.name.chars().any(char::is_control)
            || self.source.trim().is_empty()
            || self.source.len() > 200
            || self.source.chars().any(char::is_control)
        {
            return Err("Invalid synth preset version/name/source".into());
        }
        self.parameters.validate()
    }
    pub fn factory(index: usize) -> Result<Self, String> {
        let mut p = SynthParameters::default();
        let name = match index {
            0 => "Legacy Sine",
            1 => {
                p.waveform = Waveform::Saw;
                p.shape = 0.8;
                p.attack = 0.02;
                p.release = 0.2;
                "Warm Saw"
            }
            2 => {
                p.waveform = Waveform::Square;
                p.pulse_width = 0.25;
                p.attack = 0.008;
                p.decay = 0.2;
                p.sustain = 0.5;
                p.release = 0.15;
                "Hollow Pulse"
            }
            3 => {
                p.waveform = Waveform::Triangle;
                p.attack = 0.08;
                p.decay = 0.3;
                p.sustain = 0.65;
                p.release = 0.5;
                "Soft Triangle"
            }
            4 => {
                p.waveform = Waveform::Saw;
                p.shape = 0.65;
                p.attack = 0.002;
                p.decay = 0.3;
                p.sustain = 0.0;
                p.release = 0.08;
                "Short Pluck"
            }
            _ => return Err("Unknown factory preset".into()),
        };
        Ok(Self {
            version: 1,
            name: name.into(),
            source: "OpenDAW factory".into(),
            parameters: p,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_parameters_and_presets() {
        for field in [
            SynthField::Shape,
            SynthField::PulseWidth,
            SynthField::Attack,
            SynthField::Decay,
            SynthField::Sustain,
            SynthField::Release,
        ] {
            for value in [f32::NAN, f32::INFINITY, -1.0, 100.0] {
                assert!(SynthParameters::default().change(field, value).is_err());
            }
        }
        assert!(SynthParameters::default()
            .change(SynthField::Waveform, 0.5)
            .is_err());
        for i in 0..5 {
            let p = SynthPreset::factory(i).unwrap();
            p.validate().unwrap();
            assert_eq!(
                serde_json::from_slice::<SynthPreset>(&serde_json::to_vec(&p).unwrap()).unwrap(),
                p
            );
        }
        let mut json = serde_json::to_value(SynthPreset::factory(0).unwrap()).unwrap();
        json["version"] = 2.into();
        assert!(serde_json::from_value::<SynthPreset>(json.clone())
            .unwrap()
            .validate()
            .is_err());
        json["unknown"] = true.into();
        assert!(serde_json::from_value::<SynthPreset>(json).is_err());
    }
}
