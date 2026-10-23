//! Owned drum kits, embedded verified factory WAVs and content integrity. No runtime downloads.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{borrow::Cow, collections::HashSet, fs::File, io::Read, path::Path};
pub const MAX_PADS: usize = 16;
pub const ASSET_BYTE_LIMIT: usize = 128 * 1024 * 1024;
pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub struct FactoryAsset {
    pub id: &'static str,
    pub name: &'static str,
    pub midi: u8,
    pub hash: &'static str,
    pub bytes: &'static [u8],
    pub gain: f32,
}
pub static FACTORY_ASSETS: [FactoryAsset; 12] = [
    FactoryAsset {
        id: "kick",
        name: "Kick",
        midi: 35,
        hash: "642aaf3375eaff1307973f7de3ccbf74d18e71fb3cf9412e021b08fc3bf1981c",
        bytes: include_bytes!("../../assets/drums/vcsl/kick.wav"),
        gain: 0.76164045,
    },
    FactoryAsset {
        id: "snare",
        name: "Snare",
        midi: 38,
        hash: "ca0ee01347176a65f9d7e361203a61e736fa14acb2a743df355a4bded1ee40bd",
        bytes: include_bytes!("../../assets/drums/vcsl/snare.wav"),
        gain: 1.55650314,
    },
    FactoryAsset {
        id: "hat_closed",
        name: "Closed Hat",
        midi: 42,
        hash: "b65ca06ea4f8b5209fc8d98912094be1212911f21e01bd949cc9960db049d37c",
        bytes: include_bytes!("../../assets/drums/vcsl/hat_closed.wav"),
        gain: 1.50418066,
    },
    FactoryAsset {
        id: "hat_pedal",
        name: "Pedal Hat",
        midi: 44,
        hash: "4e4dbb4ddbeea653f95c2c229685fd5966ed9492fb8489d89d19dbd864f1a6b4",
        bytes: include_bytes!("../../assets/drums/vcsl/hat_pedal.wav"),
        gain: 2.50409038,
    },
    FactoryAsset {
        id: "hat_open",
        name: "Open Hat",
        midi: 46,
        hash: "fa492ec14cc6924b43ad5fcf9e9a0f406d926998835bde2eb5847cd15ad25be7",
        bytes: include_bytes!("../../assets/drums/vcsl/hat_open.wav"),
        gain: 0.88776905,
    },
    FactoryAsset {
        id: "tom_low",
        name: "Low Tom",
        midi: 45,
        hash: "bdc312f2f4c799cb43319a7aa0e664bf6194c6f8b14151051b375968df55eab4",
        bytes: include_bytes!("../../assets/drums/vcsl/tom_low.wav"),
        gain: 1.23608974,
    },
    FactoryAsset {
        id: "tom_high",
        name: "High Tom",
        midi: 50,
        hash: "40bd5cf904ed20eb8e7bba6298e356b66cedd313b4f459c5a5884c054bf447a4",
        bytes: include_bytes!("../../assets/drums/vcsl/tom_high.wav"),
        gain: 1.56982051,
    },
    FactoryAsset {
        id: "crash",
        name: "Crash",
        midi: 49,
        hash: "3b4a6ed42627eedca39f6480e9825dedeaab538197fc734f85be13ba655aa6aa",
        bytes: include_bytes!("../../assets/drums/vcsl/crash.wav"),
        gain: 0.45803641,
    },
    FactoryAsset {
        id: "tambourine",
        name: "Tambourine",
        midi: 54,
        hash: "885ebf1d70f8e888f576682e4e4112b92b69649162f392c83b3379c91a09cf41",
        bytes: include_bytes!("../../assets/drums/vcsl/tambourine.wav"),
        gain: 1.66073371,
    },
    FactoryAsset {
        id: "cowbell",
        name: "Cowbell",
        midi: 56,
        hash: "e212cc27b3490d7927d41508bd7e90d6e09b0b8da169f0357341d3057c4910c4",
        bytes: include_bytes!("../../assets/drums/vcsl/cowbell.wav"),
        gain: 0.42339917,
    },
    FactoryAsset {
        id: "bongo_high",
        name: "High Bongo",
        midi: 60,
        hash: "dd9b6239a151702966a995937f74ac022c9bb950c967277fbd16beaced0c9e1a",
        bytes: include_bytes!("../../assets/drums/vcsl/bongo_high.wav"),
        gain: 2.68101967,
    },
    FactoryAsset {
        id: "claves",
        name: "Claves",
        midi: 75,
        hash: "bcabe27330761309eb4b4f6207e6e3b4be59815df86449ac05bfb9b81f19efc9",
        bytes: include_bytes!("../../assets/drums/vcsl/claves.wav"),
        gain: 4.00000000,
    },
];
pub fn factory_asset(id: &str) -> Result<&'static FactoryAsset, String> {
    FACTORY_ASSETS
        .iter()
        .find(|a| a.id == id)
        .ok_or_else(|| "Unknown factory drum asset".into())
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DrumSample {
    Factory {
        asset_id: String,
        sha256: String,
    },
    File {
        sample_path: String,
        sha256: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        origin: Option<String>,
    },
}
impl DrumSample {
    pub fn hash(&self) -> &str {
        match self {
            Self::Factory { sha256, .. } | Self::File { sha256, .. } => sha256,
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.hash().len() != 64
            || !self
                .hash()
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err("Invalid sample SHA-256".into());
        }
        match self {
            Self::Factory { asset_id, sha256 } => {
                if factory_asset(asset_id)?.hash != sha256 {
                    return Err("Factory sample hash mismatch".into());
                }
            }
            Self::File {
                sample_path,
                origin,
                ..
            } => {
                if sample_path.is_empty() || sample_path.len() > 4096 || sample_path.contains('\0')
                {
                    return Err("Invalid drum sample path".into());
                }
                if let Some(id) = origin {
                    if factory_asset(id)?.hash != self.hash() {
                        return Err("Invalid factory provenance".into());
                    }
                }
            }
        }
        Ok(())
    }
    pub fn bytes(&self) -> Result<Cow<'static, [u8]>, String> {
        self.validate()?;
        let bytes = match self {
            Self::Factory { asset_id, .. } => Cow::Borrowed(factory_asset(asset_id)?.bytes),
            Self::File { sample_path, .. } => {
                let file = File::open(sample_path).map_err(|e| e.to_string())?;
                if file.metadata().map_err(|e| e.to_string())?.len() > ASSET_BYTE_LIMIT as u64 {
                    return Err("Drum WAV exceeds 128 MiB byte budget".into());
                }
                let mut data = Vec::new();
                file.take(ASSET_BYTE_LIMIT as u64 + 1)
                    .read_to_end(&mut data)
                    .map_err(|e| e.to_string())?;
                if data.len() > ASSET_BYTE_LIMIT {
                    return Err("Drum WAV grew beyond its budget".into());
                }
                Cow::Owned(data)
            }
        };
        if sha256(&bytes) != self.hash() {
            return Err("Drum sample SHA-256 mismatch; prior instrument retained".into());
        }
        Ok(bytes)
    }
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let path = path.canonicalize().map_err(|e| e.to_string())?;
        let file = File::open(&path).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        file.take(ASSET_BYTE_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > ASSET_BYTE_LIMIT {
            return Err("Drum WAV exceeds 128 MiB byte budget".into());
        }
        Ok(Self::File {
            sample_path: path.to_str().ok_or("Sample path must be UTF-8")?.into(),
            sha256: sha256(&bytes),
            origin: None,
        })
    }
    pub fn display(&self) -> String {
        match self {
            Self::Factory { asset_id, .. } => format!(
                "CC0 · {}",
                factory_asset(asset_id).map_or("Unknown", |a| a.name)
            ),
            Self::File {
                origin: Some(asset_id),
                ..
            } => format!(
                "CC0 · {}",
                factory_asset(asset_id).map_or("Unknown", |a| a.name)
            ),
            Self::File { .. } => "Imported WAV · included when you Save".into(),
        }
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PadParameters {
    pub gain: f32,
    pub pan: f32,
    pub tune_semitones: f32,
    pub mute: bool,
    pub one_shot: bool,
    pub choke_group: u8,
}
impl Default for PadParameters {
    fn default() -> Self {
        Self {
            gain: 1.,
            pan: 0.,
            tune_semitones: 0.,
            mute: false,
            one_shot: true,
            choke_group: 0,
        }
    }
}
impl PadParameters {
    pub fn validate(&self) -> Result<(), String> {
        if !self.gain.is_finite()
            || !(0.0..=4.).contains(&self.gain)
            || !self.pan.is_finite()
            || !(-1.0..=1.).contains(&self.pan)
            || !self.tune_semitones.is_finite()
            || !(-24.0..=24.).contains(&self.tune_semitones)
            || self.choke_group > 16
        {
            return Err("Invalid drum pad parameters".into());
        }
        Ok(())
    }
    pub fn change(&mut self, field: PadField, value: f32) -> Result<(), String> {
        if !value.is_finite() {
            return Err("Drum parameter must be finite".into());
        }
        match field {
            PadField::Gain => self.gain = value,
            PadField::Pan => self.pan = value,
            PadField::Tune => self.tune_semitones = value,
            PadField::Mute => {
                self.mute = match value {
                    0. => false,
                    1. => true,
                    _ => return Err("Invalid mute".into()),
                }
            }
            PadField::OneShot => {
                self.one_shot = match value {
                    0. => false,
                    1. => true,
                    _ => return Err("Invalid one-shot".into()),
                }
            }
            PadField::Choke => {
                if value.fract() != 0. || !(0.0..=16.).contains(&value) {
                    return Err("Invalid choke group".into());
                }
                self.choke_group = value as u8;
            }
            _ => return Err("Not a realtime drum parameter".into()),
        }
        self.validate()
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DrumPad {
    pub id: u64,
    pub name: String,
    pub midi_note: u8,
    pub sample: Option<DrumSample>,
    pub parameters: PadParameters,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DrumKit {
    pub name: String,
    pub pads: Vec<DrumPad>,
}
fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && name.len() <= 120 && !name.chars().any(char::is_control)
}
impl DrumKit {
    pub fn validate(&self) -> Result<(), String> {
        if !valid_name(&self.name) || self.pads.is_empty() || self.pads.len() > MAX_PADS {
            return Err("Drum kit requires 1..16 pads and a name".into());
        }
        let mut notes = HashSet::new();
        let mut ids = HashSet::new();
        for p in &self.pads {
            if p.id == 0
                || p.id >= (1u64 << 63)
                || !ids.insert(p.id)
                || p.midi_note > 127
                || !notes.insert(p.midi_note)
                || !valid_name(&p.name)
            {
                return Err("Invalid drum pad identity/name or duplicate MIDI mapping".into());
            }
            p.parameters.validate()?;
            if let Some(s) = &p.sample {
                s.validate()?;
            }
        }
        Ok(())
    }
    pub fn factory(mut id: impl FnMut() -> Result<u64, String>) -> Result<Self, String> {
        let mut pads = Vec::new();
        for a in &FACTORY_ASSETS {
            let mut parameters = PadParameters::default();
            parameters.gain = a.gain;
            parameters.choke_group = if matches!(a.midi, 42 | 44 | 46) { 1 } else { 0 };
            pads.push(DrumPad {
                id: id()?,
                name: a.name.into(),
                midi_note: a.midi,
                sample: Some(DrumSample::Factory {
                    asset_id: a.id.into(),
                    sha256: a.hash.into(),
                }),
                parameters,
            });
        }
        for (midi, name) in [
            (36, "Kick 2"),
            (39, "Hand Clap"),
            (40, "Electric Snare"),
            (51, "Ride"),
        ] {
            pads.push(DrumPad {
                id: id()?,
                name: name.into(),
                midi_note: midi,
                sample: None,
                parameters: PadParameters::default(),
            });
        }
        Ok(Self {
            name: "VCSL Percussion · CC0".into(),
            pads,
        })
    }
    pub fn pad(&self, id: u64) -> Option<&DrumPad> {
        self.pads.iter().find(|p| p.id == id)
    }
    pub fn note_label(&self, pitch: u8) -> String {
        self.pads
            .iter()
            .find(|p| p.midi_note == pitch)
            .map_or_else(|| format!("{pitch} · —"), |p| format!("{pitch} {}", p.name))
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadField {
    Gain,
    Pan,
    Tune,
    Mute,
    OneShot,
    Choke,
    Name,
    MidiNote,
    Sample,
}
impl PadField {
    pub fn from_index(i: i32) -> Result<Self, String> {
        match i {
            0 => Ok(Self::Gain),
            1 => Ok(Self::Pan),
            2 => Ok(Self::Tune),
            3 => Ok(Self::Mute),
            4 => Ok(Self::OneShot),
            5 => Ok(Self::Choke),
            _ => Err("Unknown pad parameter".into()),
        }
    }
    pub fn realtime(self) -> bool {
        matches!(
            self,
            Self::Gain | Self::Pan | Self::Tune | Self::Mute | Self::OneShot | Self::Choke
        )
    }
}
