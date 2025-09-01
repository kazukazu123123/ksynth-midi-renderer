use rand::Rng;
use std::f32::consts::PI;

// Target RMS level for volume normalization (0.0 to 1.0)
const TARGET_RMS: f32 = 0.3;
const TARGET_PEAK: f32 = 0.8;

// Helper function for applying ADSR envelope
fn adsr_envelope(
    t: f32,
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
    duration: f32,
) -> f32 {
    if t < attack {
        t / attack
    } else if t < attack + decay {
        1.0 - (1.0 - sustain) * ((t - attack) / decay)
    } else if t < duration - release {
        sustain
    } else if t < duration {
        sustain * (1.0 - (t - (duration - release)) / release)
    } else {
        0.0
    }
}

// Simple band-limited noise generator
fn filtered_noise(rng: &mut impl Rng, low_freq: f32, high_freq: f32, t: f32) -> f32 {
    let white_noise = rng.random_range(-1.0..1.0);
    let freq_factor = (high_freq - low_freq) * rng.random::<f32>() + low_freq;
    white_noise * (2.0 * PI * freq_factor * t).sin()
}

// Normalize audio samples to consistent volume
fn normalize_samples(samples: &mut [f32]) {
    if samples.is_empty() {
        return;
    }

    let rms = (samples.iter().map(|&x| x * x).sum::<f32>() / samples.len() as f32).sqrt();
    let peak = samples.iter().map(|&x| x.abs()).fold(0.0f32, f32::max);

    if rms > 0.0 && peak > 0.0 {
        let rms_scale = TARGET_RMS / rms;
        let peak_scale = TARGET_PEAK / peak;
        let scale = rms_scale.min(peak_scale);
        for sample in samples.iter_mut() {
            *sample *= scale;
        }
    }
}

// Convert normalized f32 samples to i16
fn samples_to_i16(samples: Vec<f32>) -> Vec<i16> {
    samples
        .into_iter()
        .map(|s| (s * i16::MAX as f32).clamp(i16::MIN as f32, i16::MAX as f32) as i16)
        .collect()
}

const SILENCE_THRESHOLD: i16 = 50; // Threshold for silence detection

fn trim_silence(mut samples: Vec<i16>, sample_rate: u32) -> Vec<i16> {
    // Trim trailing silence
    let mut end_index = samples.len();
    for i in (0..samples.len()).rev() {
        if samples[i].abs() > SILENCE_THRESHOLD {
            end_index = i + 1;
            break;
        }
    }
    samples.truncate(end_index);

    // If after trimming, the sample is too short, ensure a minimum length (e.g., 10ms)
    let min_length_samples = (sample_rate as f32 * 0.01) as usize;
    if samples.len() < min_length_samples && !samples.is_empty() {
        samples.resize(min_length_samples, 0);
    }
    samples
}

// -------------------- Drum Generators -------------------- //

pub fn generate_kick_sample(sample_rate: u32, sample_count: usize) -> Vec<i16> {
    let mut float_samples = Vec::with_capacity(sample_count);
    let mut rng = rand::rng();
    let fundamental_freq = 35.0;
    let overtone_freq = 80.0;

    for i in 0..sample_count {
        let t = i as f32 / sample_rate as f32;
        let pitch_bend = (-15.0 * t).exp();
        let current_fundamental = fundamental_freq * (1.0 + 2.0 * pitch_bend);
        let main_envelope = (-8.0 * t).exp();
        let click_envelope = (-80.0 * t).exp();
        let sub_envelope = (-3.0 * t).exp();

        let fundamental = (2.0 * PI * current_fundamental * t).sin() * main_envelope * 0.8;
        let sub_bass = (2.0 * PI * current_fundamental * 0.5 * t).sin() * sub_envelope * 0.4;
        let overtone = (2.0 * PI * overtone_freq * t).sin() * main_envelope * 0.2;
        let click_noise = filtered_noise(&mut rng, 2000.0, 5000.0, t) * click_envelope * 0.3;
        let modulation = (2.0 * PI * 8.0 * t).sin() * 0.1 * main_envelope;

        let sample = (fundamental + sub_bass + overtone + click_noise) * (1.0 + modulation);
        float_samples.push(sample.tanh());
    }

    normalize_samples(&mut float_samples);
    samples_to_i16(float_samples)
}

pub fn generate_snare_sample(sample_rate: u32, sample_count: usize) -> Vec<i16> {
    let mut float_samples = Vec::with_capacity(sample_count);
    let mut rng = rand::rng();
    let duration = sample_count as f32 / sample_rate as f32;

    for i in 0..sample_count {
        let t = i as f32 / sample_rate as f32;
        let envelope = adsr_envelope(t, 0.001, 0.05, 0.3, 0.1, duration);
        let drum_head = (2.0 * PI * 200.0 * t).sin() * envelope * 0.4;

        let mut snare_buzz = 0.0;
        for _ in 0..8 {
            let buzz_freq = rng.random_range(150.0..400.0);
            snare_buzz += (2.0 * PI * buzz_freq * t).sin() * rng.random_range(0.5..1.0);
        }
        snare_buzz = snare_buzz / 8.0 * envelope * 0.6;

        let stick_attack = filtered_noise(&mut rng, 2000.0, 8000.0, t) * (-25.0 * t).exp() * 0.4;
        let rim_component = (2.0 * PI * 1200.0 * t).sin() * (-40.0 * t).exp() * 0.2;
        let shell_resonance = (2.0 * PI * 250.0 * t).sin() * envelope * 0.1;

        let sample =
            (drum_head + snare_buzz + stick_attack + rim_component + shell_resonance).tanh();
        float_samples.push(sample);
    }

    normalize_samples(&mut float_samples);
    samples_to_i16(float_samples)
}

pub fn generate_hihat_sample(sample_rate: u32, sample_count: usize) -> Vec<i16> {
    let mut float_samples = Vec::with_capacity(sample_count);
    let mut rng = rand::rng();
    let freqs = [300.0, 450.0, 680.0, 920.0, 1200.0, 1600.0];

    for i in 0..sample_count {
        let t = i as f32 / sample_rate as f32;
        let envelope = (-20.0 * t).exp();
        let mut sample = 0.0;

        for (idx, &freq) in freqs.iter().enumerate() {
            let harmonic_envelope = (-(10.0 + idx as f32 * 3.0) * t).exp();
            let freq_mod = 1.0 + 0.1 * (2.0 * PI * 30.0 * t).sin();
            sample += (2.0 * PI * freq * freq_mod * t).sin() * harmonic_envelope / (idx + 1) as f32;
        }

        let sizzle = filtered_noise(&mut rng, 6000.0, 12000.0, t) * envelope * 0.3;
        let attack_transient =
            filtered_noise(&mut rng, 8000.0, 15000.0, t) * (-100.0 * t).exp() * 0.4;
        float_samples.push((sample * 0.6 + sizzle + attack_transient) * envelope);
    }

    normalize_samples(&mut float_samples);
    samples_to_i16(float_samples)
}

pub fn generate_ride_cymbal_sample(sample_rate: u32, sample_count: usize) -> Vec<i16> {
    let mut samples = Vec::with_capacity(sample_count);
    let mut rng = rand::rng();
    let bell_freq = 2000.0;
    let body_freq = 400.0;

    for i in 0..sample_count {
        let t = i as f32 / sample_rate as f32;
        let bell_envelope = (-8.0 * t).exp() + (-1.0 * t).exp() * 0.3;
        let body_envelope = (-4.0 * t).exp();
        let bell_fundamental = (2.0 * PI * bell_freq * t).sin() * bell_envelope * 0.5;
        let bell_harmonic = (2.0 * PI * bell_freq * 1.5 * t).sin() * bell_envelope * 0.2;

        let mut body_sample = 0.0;
        for harmonic in 1..=5 {
            let freq = body_freq * harmonic as f32 * (1.0 + 0.1 * rng.random::<f32>());
            body_sample += (2.0 * PI * freq * t).sin() / harmonic as f32;
        }
        body_sample *= body_envelope * 0.3;
        let stick_attack = filtered_noise(&mut rng, 3000.0, 8000.0, t) * (-30.0 * t).exp() * 0.2;
        let sample = bell_fundamental + bell_harmonic + body_sample + stick_attack;
        samples.push((sample * i16::MAX as f32).clamp(-i16::MAX as f32, i16::MAX as f32) as i16);
    }

    trim_silence(samples, sample_rate)
}

pub fn generate_hand_clap_sample(sample_rate: u32, sample_count: usize) -> Vec<i16> {
    let mut samples = Vec::with_capacity(sample_count);
    let mut rng = rand::rng();

    for i in 0..sample_count {
        let t = i as f32 / sample_rate as f32;
        let mut clap_sample = 0.0;

        let clap_events = [
            (0.0, 1.0, 0.8),
            (0.002, 0.85, 0.7),
            (0.005, 0.9, 0.75),
            (0.008, 0.7, 0.6),
            (0.012, 0.8, 0.65),
            (0.016, 0.6, 0.5),
        ];

        for &(clap_time, intensity, decay_rate) in &clap_events {
            if t >= clap_time {
                let clap_t = t - clap_time;
                let clap_envelope = (-30.0 * decay_rate * clap_t).exp();

                let attack_transient = filtered_noise(&mut rng, 2000.0, 8000.0, clap_t)
                    * (-150.0 * clap_t).exp()
                    * 0.6;
                let palm_resonance =
                    filtered_noise(&mut rng, 800.0, 2500.0, clap_t) * clap_envelope * 0.7;
                let body_thump =
                    filtered_noise(&mut rng, 200.0, 600.0, clap_t) * (-8.0 * clap_t).exp() * 0.3;
                let air_pop = (2.0 * PI * 1200.0 * clap_t).sin() * (-60.0 * clap_t).exp() * 0.4;
                let finger_slap =
                    filtered_noise(&mut rng, 3000.0, 6000.0, clap_t) * (-80.0 * clap_t).exp() * 0.5;

                let reflection_delay_samples = (0.0008 * sample_rate as f32) as usize;
                let reflection_component = if i >= reflection_delay_samples {
                    let reflected_t =
                        clap_t - (reflection_delay_samples as f32 / sample_rate as f32);
                    if reflected_t >= 0.0 {
                        filtered_noise(&mut rng, 1000.0, 4000.0, reflected_t)
                            * (-40.0 * reflected_t).exp()
                            * 0.15
                    } else {
                        0.0
                    }
                } else {
                    0.0
                };

                let mut clap_component = attack_transient
                    + palm_resonance
                    + body_thump
                    + air_pop
                    + finger_slap
                    + reflection_component;
                let pitch_variation = 1.0 + (rng.random::<f32>() - 0.5) * 0.1;
                clap_component *= pitch_variation;
                let intensity_variation = intensity * (0.9 + rng.random::<f32>() * 0.2);
                clap_sample += clap_component * intensity_variation;
            }
        }

        clap_sample *= 1.0 + (rng.random::<f32>() - 0.5) * 0.05;
        clap_sample = clap_sample.tanh() * 0.8;
        samples
            .push((clap_sample * i16::MAX as f32).clamp(-i16::MAX as f32, i16::MAX as f32) as i16);
    }

    trim_silence(samples, sample_rate)
}

pub fn generate_acoustic_bass_drum_sample(sample_rate: u32, sample_count: usize) -> Vec<i16> {
    let mut float_samples = Vec::with_capacity(sample_count);
    let mut rng = rand::rng();
    let fundamental_freq = 40.0; // Slightly higher fundamental for acoustic feel
    let overtone_freq = 90.0; // Slightly higher overtone

    for i in 0..sample_count {
        let t = i as f32 / sample_rate as f32;
        let pitch_bend = (-10.0 * t).exp(); // Less aggressive pitch bend
        let current_fundamental = fundamental_freq * (1.0 + 1.5 * pitch_bend); // Less dramatic pitch bend
        let main_envelope = (-6.0 * t).exp(); // Longer decay
        let click_envelope = (-60.0 * t).exp(); // Softer click
        let sub_envelope = (-2.0 * t).exp(); // Longer sub decay

        let fundamental = (2.0 * PI * current_fundamental * t).sin() * main_envelope * 0.9; // Stronger fundamental
        let sub_bass = (2.0 * PI * current_fundamental * 0.5 * t).sin() * sub_envelope * 0.5; // Stronger sub
        let overtone = (2.0 * PI * overtone_freq * t).sin() * main_envelope * 0.3; // More prominent overtone
        let click_noise = filtered_noise(&mut rng, 1500.0, 4000.0, t) * click_envelope * 0.2; // Softer, lower frequency click
        let modulation = (2.0 * PI * 6.0 * t).sin() * 0.05 * main_envelope; // Less modulation

        let sample = (fundamental + sub_bass + overtone + click_noise) * (1.0 + modulation);
        float_samples.push(sample.tanh());
    }

    normalize_samples(&mut float_samples);
    samples_to_i16(float_samples)
}

pub fn generate_side_stick_sample(sample_rate: u32, sample_count: usize) -> Vec<i16> {
    let mut float_samples = Vec::with_capacity(sample_count);
    let mut rng = rand::rng();
    let duration = sample_count as f32 / sample_rate as f32;

    for i in 0..sample_count {
        let t = i as f32 / sample_rate as f32;
        let envelope = adsr_envelope(t, 0.001, 0.02, 0.0, 0.05, duration); // Very short, sharp envelope

        let wood_click = filtered_noise(&mut rng, 3000.0, 8000.0, t) * (-100.0 * t).exp() * 0.8; // Sharp, high-frequency click
        let rim_resonance = (2.0 * PI * 800.0 * t).sin() * (-50.0 * t).exp() * 0.4; // Resonant rim sound
        let shell_thump = (2.0 * PI * 150.0 * t).sin() * (-30.0 * t).exp() * 0.2; // Low thump from shell

        let sample = (wood_click + rim_resonance + shell_thump) * envelope;
        float_samples.push(sample.tanh());
    }

    normalize_samples(&mut float_samples);
    samples_to_i16(float_samples)
}

pub fn generate_electric_snare_sample(sample_rate: u32, sample_count: usize) -> Vec<i16> {
    let mut float_samples = Vec::with_capacity(sample_count);
    let mut rng = rand::rng();
    let duration = sample_count as f32 / sample_rate as f32;

    for i in 0..sample_count {
        let t = i as f32 / sample_rate as f32;
        let envelope = adsr_envelope(t, 0.001, 0.08, 0.2, 0.1, duration); // Slightly longer decay for electronic feel

        let body_tone = (2.0 * PI * 180.0 * t).sin() * envelope * 0.5; // Lower fundamental tone
        let noise_component = filtered_noise(&mut rng, 500.0, 5000.0, t) * envelope * 0.7; // Broader noise spectrum
        let snap_attack = filtered_noise(&mut rng, 4000.0, 10000.0, t) * (-30.0 * t).exp() * 0.6; // Sharp, high-frequency attack

        let sample = (body_tone + noise_component + snap_attack).tanh();
        float_samples.push(sample);
    }

    normalize_samples(&mut float_samples);
    samples_to_i16(float_samples)
}

pub fn generate_pedal_hihat_sample(sample_rate: u32, sample_count: usize) -> Vec<i16> {
    let mut float_samples = Vec::with_capacity(sample_count);
    let mut rng = rand::rng();
    let freqs = [200.0, 300.0, 450.0, 600.0, 800.0]; // Lower frequencies for closed hi-hat

    for i in 0..sample_count {
        let t = i as f32 / sample_rate as f32;
        let envelope = (-30.0 * t).exp(); // Very short decay for closed sound
        let mut sample = 0.0;

        for (idx, &freq) in freqs.iter().enumerate() {
            let harmonic_envelope = (-(15.0 + idx as f32 * 5.0) * t).exp();
            sample += (2.0 * PI * freq * t).sin() * harmonic_envelope / (idx + 1) as f32;
        }

        let click_noise = filtered_noise(&mut rng, 1000.0, 5000.0, t) * (-80.0 * t).exp() * 0.4; // Sharp click from pedal
        float_samples.push((sample * 0.7 + click_noise) * envelope);
    }

    normalize_samples(&mut float_samples);
    samples_to_i16(float_samples)
}

pub fn generate_crash_cymbal_sample(sample_rate: u32, sample_count: usize) -> Vec<i16> {
    let mut float_samples = Vec::with_capacity(sample_count);
    let mut rng = rand::rng();

    let freqs = [300.0, 500.0, 800.0, 1200.0, 1800.0, 2500.0]; // 金属的ハーモニクス

    for i in 0..sample_count {
        let t = i as f32 / sample_rate as f32;

        // 長く減衰するハーモニクス
        let mut harmonics = 0.0;
        for (idx, &freq) in freqs.iter().enumerate() {
            let env = (-(2.0 + idx as f32 * 0.5) * t).exp(); // ゆっくり減衰
            harmonics += (2.0 * PI * freq * t).sin() * env / (idx + 1) as f32;
        }

        // 高周波シズルノイズ
        let sizzle = filtered_noise(&mut rng, 5000.0, 20000.0, t) * (-2.5 * t).exp() * 0.5;

        let sample = (harmonics * 0.6 + sizzle).tanh();
        float_samples.push(sample);
    }

    normalize_samples(&mut float_samples);
    samples_to_i16(float_samples)
}
