use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use ksynth_core::{Channel, KSynth, drum_kit::DrumKit, sample::Sample};
use rayon::iter::{IntoParallelRefMutIterator, ParallelIterator};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct NoteKey {
    channel: u8,
    note: u8,
}

pub struct MultiSynth {
    synths: Vec<KSynth>,
    note_map: HashMap<NoteKey, Vec<usize>>, // Note key -> list of instance indices
    note_counts: Vec<u32>,                  // Current number of simultaneous voices per instance
    max_voices: Vec<u32>,                   // Maximum number of simultaneous voices per instance
    drum_synth_idx: Option<usize>,          // Index of the KSynth instance that holds the DrumKit
}

impl MultiSynth {
    pub fn new(
        sample_rate: u32,
        num_channel: Channel,
        max_total_voices: u32,
        fade_out_sample: u64,
        sample_map: Arc<RwLock<HashMap<u8, Sample>>>,
        mut drum_kit: Option<DrumKit>, // Changed to mut Option<DrumKit>
        num_instances: usize,
    ) -> Self {
        let base_voice_count = max_total_voices / num_instances as u32;
        let mut max_voices = vec![base_voice_count; num_instances];

        for i in 0..(max_total_voices % num_instances as u32) {
            max_voices[i as usize] += 1;
        }

        let mut synths = Vec::with_capacity(num_instances);
        let mut filtered_max_voices = Vec::new();
        let mut drum_synth_idx: Option<usize> = None;

        for (i, &voices) in max_voices.iter().enumerate() {
            // Added enumerate
            if voices > 0 {
                let current_drum_kit = if i == 0 {
                    drum_synth_idx = Some(i); // Store the index
                    drum_kit.take()
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

        let synth_len = synths.len();

        MultiSynth {
            synths,
            note_map: HashMap::new(),
            note_counts: vec![0; synth_len],
            max_voices: filtered_max_voices,
            drum_synth_idx, // Initialize drum_synth_idx
        }
    }

    pub fn queue_midi_cmd(&mut self, cmd: u32) {
        let status = (cmd & 0xFF) as u8;
        let note = ((cmd >> 8) & 0xFF) as u8;
        let velocity = ((cmd >> 16) & 0xFF) as u8;

        let channel = status & 0x0F;
        let status_nibble = status & 0xF0;

        // Check if it's a drum channel event (MIDI Channel 9 is 0x09)
        if channel == 0x09 && self.drum_synth_idx.is_some() {
            // Route drum events to the dedicated drum synth
            let idx = self.drum_synth_idx.unwrap();
            match status_nibble {
                0x90 => {
                    if velocity == 0 {
                        self.synths[idx].queue_midi_cmd(cmd); // Note off
                    } else {
                        self.synths[idx].queue_midi_cmd(cmd); // Note on
                    }
                }
                0x80 => self.synths[idx].queue_midi_cmd(cmd), // Note off
                _ => {} // Other MIDI messages for drum channel can be ignored or handled as needed
            }
        } else {
            // For non-drum channels, use existing polyphony-based distribution
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
        if let Some((idx, _)) = self
            .note_counts
            .iter()
            .enumerate()
            .filter(|&(i, &count)| count < self.max_voices[i])
            .min_by_key(|&(_, &count)| count)
        {
            let note_key = NoteKey { channel, note };

            self.synths[idx].queue_midi_cmd(cmd);
            self.note_map.entry(note_key).or_default().push(idx);
            self.note_counts[idx] += 1;
        }
    }

    fn note_off(&mut self, channel: u8, note: u8, cmd: u32) {
        let note_key = NoteKey { channel, note };

        if let Some(indices) = self.note_map.get_mut(&note_key) {
            if let Some(idx) = indices.pop() {
                self.synths[idx].queue_midi_cmd(cmd);
                if self.note_counts[idx] > 0 {
                    self.note_counts[idx] -= 1;
                }
            }
            if indices.is_empty() {
                self.note_map.remove(&note_key);
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
        let sum: f32 = self.synths.iter().map(|s| s.get_rendering_time()).sum();
        let count = self.synths.len();
        if count == 0 { 0.0 } else { sum / count as f32 }
    }
}
