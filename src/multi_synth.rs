use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use ksynth_core::{Channel, KSynth, drum_kit::DrumKit, sample::Sample};
use num_cpus;
use rayon::iter::{IntoParallelRefMutIterator, ParallelIterator};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct NoteKey {
    channel: u8,
    note: u8,
}

pub struct MultiSynth {
    synths: Vec<KSynth>,
    note_map: HashMap<NoteKey, usize>, // Note key -> instance index
    note_counts: Vec<u32>,             // Current number of simultaneous voices per instance
    max_voices: Vec<u32>,              // Maximum number of simultaneous voices per instance
    drum_kit_storage: Option<DrumKit>,
    sample_rate: u32,
    num_channel: Channel,
    fade_out_sample: u64,
    sample_map: Arc<RwLock<HashMap<u8, Sample>>>,
    max_total_voices: u32,
}

impl MultiSynth {
    fn build_synths(
        sample_rate: u32,
        num_channel: Channel,
        max_total_voices: u32,
        fade_out_sample: u64,
        sample_map: Arc<RwLock<HashMap<u8, Sample>>>,
        drum_kit: Option<DrumKit>,
        mut num_instances: usize,
    ) -> (Vec<KSynth>, Vec<u32>) {
        let max_threads = num_cpus::get();
        if num_instances > max_threads {
            num_instances = max_threads;
        }

        if num_instances == 0 {
            num_instances = 1;
        }

        let base_voice_count = max_total_voices / num_instances as u32;
        let mut max_voices = vec![base_voice_count; num_instances];
        for i in 0..(max_total_voices % num_instances as u32) {
            max_voices[i as usize] += 1;
        }

        let mut synths = Vec::new();
        let mut filtered_max_voices = Vec::new();

        let drum_kit_cloned = drum_kit.clone();

        for (i, &voices) in max_voices.iter().enumerate() {
            if voices > 0 {
                let current_drum_kit = if i == 0 && drum_kit_cloned.is_some() {
                    drum_kit_cloned.clone()
                } else {
                    None
                };
                synths.push(KSynth::new(
                    sample_rate,
                    num_channel,
                    voices,
                    fade_out_sample,
                    sample_map.clone(),
                    current_drum_kit,
                ));
                filtered_max_voices.push(voices);
            }
        }

        (synths, filtered_max_voices)
    }

    pub fn new(
        sample_rate: u32,
        num_channel: Channel,
        max_total_voices: u32,
        fade_out_sample: u64,
        sample_map: Arc<RwLock<HashMap<u8, Sample>>>,
        drum_kit: Option<DrumKit>,
        num_instances: usize,
    ) -> Self {
        let (synths, filtered_max_voices) = Self::build_synths(
            sample_rate,
            num_channel,
            max_total_voices,
            fade_out_sample,
            sample_map.clone(),
            drum_kit.clone(),
            num_instances,
        );

        let synth_len = synths.len();

        MultiSynth {
            synths,
            note_map: HashMap::new(),
            note_counts: vec![0; synth_len],
            max_voices: filtered_max_voices,
            drum_kit_storage: drum_kit,
            sample_rate,
            num_channel,
            fade_out_sample,
            sample_map,
            max_total_voices,
        }
    }

    pub fn queue_midi_cmd(&mut self, cmd: u32) {
        let status = (cmd & 0xFF) as u8;
        let note = ((cmd >> 8) & 0xFF) as u8;
        let velocity = ((cmd >> 16) & 0xFF) as u8;

        let channel = status & 0x0F;
        let status_nibble = status & 0xF0;

        if channel == 0x09 && self.drum_kit_storage.is_some() {
            let idx = 0;
            match status_nibble {
                0x90 => {
                    if velocity == 0 {
                        self.synths[idx].queue_midi_cmd(cmd);
                    } else {
                        self.synths[idx].queue_midi_cmd(cmd);
                    }
                }
                0x80 => self.synths[idx].queue_midi_cmd(cmd),
                _ => {}
            }
        } else {
            match status_nibble {
                0x90 => {
                    if velocity == 0 {
                        self.note_off(channel, note, cmd);
                    } else {
                        self.note_on(channel, note, cmd);
                    }
                }
                0x80 => self.note_off(channel, note, cmd),
                0xA0..=0xEF => {
                    for synth in &mut self.synths {
                        synth.queue_midi_cmd(cmd);
                    }
                }
                _ => {}
            }
        }
    }

    fn note_on(&mut self, channel: u8, note: u8, cmd: u32) {
        let note_key = NoteKey { channel, note };

        if let Some(&old_idx) = self.note_map.get(&note_key) {
            let note_off_cmd = (0x80 | channel) as u32 | ((note as u32) << 8) | (0 << 16);
            self.synths[old_idx].queue_midi_cmd(note_off_cmd);
            if self.note_counts[old_idx] > 0 {
                self.note_counts[old_idx] -= 1;
            }
        }

        if let Some((idx, _)) = self
            .note_counts
            .iter()
            .enumerate()
            .filter(|&(i, &count)| count < self.max_voices[i])
            .min_by_key(|&(_, &count)| count)
        {
            self.synths[idx].queue_midi_cmd(cmd);
            self.note_map.insert(note_key, idx);
            self.note_counts[idx] += 1;
        }
    }

    fn note_off(&mut self, channel: u8, note: u8, cmd: u32) {
        let note_key = NoteKey { channel, note };

        if let Some(idx) = self.note_map.remove(&note_key) {
            self.synths[idx].queue_midi_cmd(cmd);
            if self.note_counts[idx] > 0 {
                self.note_counts[idx] -= 1;
            }
        }
    }

    pub fn fill_buffer(&mut self, output: &mut [f32]) {
        let len = output.len();
        let temp_buffers: Vec<Vec<f32>> = self
            .synths
            .par_iter_mut()
            .map(|synth| {
                let mut temp = vec![0.0f32; len];
                synth.fill_buffer(&mut temp);
                temp
            })
            .collect();

        output.fill(0.0);
        for buffer in &temp_buffers {
            for (o, &s) in output.iter_mut().zip(buffer.iter()) {
                *o += s;
            }
        }
    }

    pub fn get_polyphony(&self) -> u32 {
        self.synths.iter().map(|synth| synth.get_polyphony()).sum()
    }

    pub fn get_max_polyphony(&self) -> u32 {
        self.synths
            .iter()
            .map(|synth| synth.get_max_polyphony())
            .sum()
    }

    pub fn get_rendering_time_ratio(&self) -> f32 {
        self.synths
            .iter()
            .map(|s| s.get_rendering_time())
            .fold(0.0_f32, |max_val, t| t.max(max_val))
    }

    pub fn set_max_polyphony(&mut self, max_total_voices: u32) {
        self.max_total_voices = max_total_voices;

        for (note_key, &idx) in &self.note_map {
            let channel = note_key.channel;
            let note = note_key.note;
            let note_off_cmd = (0x80 | channel) as u32 | ((note as u32) << 8) | (0 << 16);
            if idx < self.synths.len() {
                self.synths[idx].queue_midi_cmd(note_off_cmd);
            }
        }

        let (new_synths, new_max_voices) = Self::build_synths(
            self.sample_rate,
            self.num_channel,
            max_total_voices,
            self.fade_out_sample,
            self.sample_map.clone(),
            self.drum_kit_storage.clone(),
            self.synths.len(),
        );

        self.synths = new_synths;
        self.max_voices = new_max_voices;
        self.note_map.clear();
        self.note_counts = vec![0; self.synths.len()];
    }

    pub fn get_num_instances(&self) -> usize {
        self.synths.len()
    }

    pub fn set_num_instances(&mut self, new_num_instances: usize) {
        for (note_key, &idx) in &self.note_map {
            let channel = note_key.channel;
            let note = note_key.note;
            let note_off_cmd = (0x80 | channel) as u32 | ((note as u32) << 8) | (0 << 16);
            if idx < self.synths.len() {
                self.synths[idx].queue_midi_cmd(note_off_cmd);
            }
        }

        let (new_synths, new_max_voices) = Self::build_synths(
            self.sample_rate,
            self.num_channel,
            self.max_total_voices,
            self.fade_out_sample,
            self.sample_map.clone(),
            self.drum_kit_storage.clone(),
            new_num_instances,
        );

        self.synths = new_synths;
        self.max_voices = new_max_voices;
        self.note_map.clear();
        self.note_counts = vec![0; self.synths.len()];
    }
}
