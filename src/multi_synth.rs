use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use ksynth_core::{Channel, KSynth, sample::Sample};
use rayon::iter::{IntoParallelRefMutIterator, ParallelIterator};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct NoteKey {
    channel: u8,
    note: u8,
}

pub struct MultiSynth {
    synths: Vec<KSynth>,
    note_map: HashMap<NoteKey, Vec<usize>>, // ノートキー → インスタンス番号リスト
    note_counts: Vec<u32>,                  // 各インスタンスの現在同時発音数
    max_voices: Vec<u32>,                   // 各インスタンスの最大同時発音数
}

impl MultiSynth {
    pub fn new(
        sample_rate: u32,
        num_channel: Channel,
        max_total_voices: u32,
        fade_out_sample: u64,
        sample_map: Arc<RwLock<HashMap<u8, Sample>>>,
        num_instances: usize,
    ) -> Self {
        let base_voice_count = max_total_voices / num_instances as u32;
        let mut max_voices = vec![base_voice_count; num_instances];

        for i in 0..(max_total_voices % num_instances as u32) {
            max_voices[i as usize] += 1;
        }

        let synths = max_voices
            .iter()
            .map(|&voices| {
                KSynth::new(
                    sample_rate,
                    num_channel,
                    voices as u32,
                    fade_out_sample,
                    sample_map.clone(),
                )
            })
            .collect();

        MultiSynth {
            synths,
            note_map: HashMap::new(),
            note_counts: vec![0; num_instances],
            max_voices,
        }
    }

    pub fn queue_midi_cmd(&mut self, cmd: u32) {
        let status = (cmd & 0xFF) as u8;
        let note = ((cmd >> 8) & 0xFF) as u8;
        let velocity = ((cmd >> 16) & 0xFF) as u8;

        let channel = status & 0x0F;
        let status_nibble = status & 0xF0;

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
