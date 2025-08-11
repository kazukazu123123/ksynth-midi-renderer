use rand::Rng;
use std::f32::consts::PI;

pub fn generate_kick_sample(sample_rate: u32, sample_count: usize) -> Vec<i16> {
    let mut samples = Vec::with_capacity(sample_count);

    let start_freq = 100.0; // Starting frequency for the pitch slide
    let base_freq = 30.0; // Target frequency after slide
    let pitch_decay_factor = 2.0; // How fast the pitch decays
    let amplitude_decay_factor = 0.008; // Faster amplitude decay for punch
    let gain = 1.2; // Increased gain

    for i in 0..sample_count {
        let t = i as f32 / sample_rate as f32;

        // Pitch envelope: exponential decay from start_freq to base_freq
        let current_freq = base_freq + (start_freq - base_freq) * (-pitch_decay_factor * t).exp();

        // Amplitude envelope
        let amplitude = (-amplitude_decay_factor * t).exp();

        let sample = (2.0 * PI * current_freq * t).sin() * amplitude * gain;

        let scaled = (sample * i16::MAX as f32)
            .clamp(-i16::MAX as f32, i16::MAX as f32) as i16;
        samples.push(scaled);
    }

    samples
}

pub fn generate_acoustic_bass_drum_sample(sample_rate: u32, sample_count: usize) -> Vec<i16> {
    let mut samples = Vec::with_capacity(sample_count);

    let start_freq = 100.0; // Starting frequency for the pitch slide
    let base_freq = 20.0; // Target frequency after slide
    let pitch_decay_factor = 0.1; // How fast the pitch decays
    let amplitude_decay_factor = 0.009; // Faster amplitude decay for punch
    let gain = 1.1; // Increased gain

    for i in 0..sample_count {
        let t = i as f32 / sample_rate as f32;

        // Pitch envelope: exponential decay from start_freq to base_freq
        let current_freq = base_freq + (start_freq - base_freq) * (-pitch_decay_factor * t).exp();

        // Amplitude envelope
        let amplitude = (-amplitude_decay_factor * t).exp();

        let sample = (2.0 * PI * current_freq * t).sin() * amplitude * gain;

        let scaled = (sample * i16::MAX as f32)
            .clamp(-i16::MAX as f32, i16::MAX as f32) as i16;
        samples.push(scaled);
    }

    samples
}


pub fn generate_snare_sample(sample_rate: u32, sample_count: usize) -> Vec<i16> {
    let mut samples = Vec::with_capacity(sample_count);
    let mut rng = rand::rng();

    let noise_duration = 0.03; // Even shorter duration
    let tone_freq = 200.0; // Frequency of the tonal component
    let tone_decay_factor = 0.025; // Even faster decay
    let noise_decay_factor = 0.03; // Even faster decay

    for i in 0..sample_count {
        let t = i as f32 / sample_rate as f32;

        // Tonal component
        let tone_amplitude = (-tone_decay_factor * t).exp();
        let tone_sample = (2.0 * PI * tone_freq * t).sin() * tone_amplitude;

        // Noise component
        let noise_amplitude = if t < noise_duration {
            (-noise_decay_factor * t).exp()
        } else {
            0.0
        };
        let noise_sample = (rng.random_range(-1.0..1.0) + rng.random_range(-1.0..1.0)) * 0.5 * noise_amplitude * 0.2; // Further reduced noise contribution

        let sample = tone_sample * 0.4 + noise_sample * 0.6; // Mix tone and noise

        let scaled = (sample * i16::MAX as f32)
            .clamp(-i16::MAX as f32, i16::MAX as f32) as i16;
        samples.push(scaled);
    }

    samples
}

pub fn generate_side_stick_sample(sample_rate: u32, sample_count: usize) -> Vec<i16> {
    let mut samples = Vec::with_capacity(sample_count);
    let mut rng = rand::rng();

    let click_decay_factor = 5.0; // Extremely fast decay for the click noise
    let click_noise_freq_start = 8000.0; // High frequency for the click noise
    let click_noise_freq_end = 16000.0; // High frequency for the click noise

    for i in 0..sample_count {
        let t = i as f32 / sample_rate as f32;

        // Click noise component (very short, high-frequency noise burst)
        let click_amplitude = (-click_decay_factor * t).exp();
        let mut click_noise_sample = 0.0;
        for _ in 0..4 { // Mix several high-frequency random values
            let freq = rng.random_range(click_noise_freq_start..click_noise_freq_end);
            click_noise_sample += (2.0 * PI * freq * t).sin();
        }
        click_noise_sample /= 4.0; // Normalize
        click_noise_sample *= click_amplitude;

        let combined_sample = click_noise_sample * 0.8; // Emphasize click noise

        let scaled = (combined_sample * i16::MAX as f32)
            .clamp(-i16::MAX as f32, i16::MAX as f32) as i16;
        samples.push(scaled);
    }

    samples
}

pub fn generate_hand_clap_sample(sample_rate: u32, sample_count: usize) -> Vec<i16> {
    let mut samples = Vec::with_capacity(sample_count);
    let mut rng = rand::rng();

    let decay_factor = 0.5; // Even faster decay
    let noise_amount = 0.3; // Further reduced noise amount

    for i in 0..sample_count {
        let t = i as f32 / sample_rate as f32;

        let amplitude = (-decay_factor * t).exp();

        let noise = rng.random_range(-1.0..1.0) * noise_amount;
        let sample = noise * amplitude;

        let scaled = (sample * i16::MAX as f32)
            .clamp(-i16::MAX as f32, i16::MAX as f32) as i16;
        samples.push(scaled);
    }

    samples
}

pub fn generate_electric_snare_sample(sample_rate: u32, sample_count: usize) -> Vec<i16> {
    let mut samples = Vec::with_capacity(sample_count);
    let mut rng = rand::rng();

    let noise_duration = 0.06; // Even shorter duration
    let tone_freq = 300.0; // Higher tonal frequency
    let tone_decay_factor = 0.03; // Even faster tonal decay
    let noise_decay_factor = 0.025; // Even faster noise decay

    for i in 0..sample_count {
        let t = i as f32 / sample_rate as f32;

        // Tonal component
        let tone_amplitude = (-tone_decay_factor * t).exp();
        let tone_sample = (2.0 * PI * tone_freq * t).sin() * tone_amplitude;

        // Noise component
        let noise_amplitude = if t < noise_duration {
            (-noise_decay_factor * t).exp()
        } else {
            0.0
        };
        let noise_sample = (rng.random_range(-1.0..1.0) + rng.random_range(-1.0..1.0)) * 0.5 * noise_amplitude * 0.5; // Reduced noise contribution

        let sample = tone_sample * 0.3 + noise_sample * 0.7; // More noise for electric snare

        let scaled = (sample * i16::MAX as f32)
            .clamp(-i16::MAX as f32, i16::MAX as f32) as i16;
        samples.push(scaled);
    }

    samples
}

pub fn generate_pedal_hihat_sample(sample_rate: u32, sample_count: usize) -> Vec<i16> {
    let mut samples = Vec::with_capacity(sample_count);
    let mut rng = rand::rng();

    let decay_factor = 0.3; // Even more significantly faster decay
    let freq_range_start = 2000.0;
    let freq_range_end = 5000.0;

    for i in 0..sample_count {
        let t = i as f32 / sample_rate as f32;

        let amplitude = (-decay_factor * t).exp();

        let mut sample = 0.0;
        for _ in 0..4 { // Mix several sine waves
            let freq = rng.random_range(freq_range_start..freq_range_end);
            sample += (2.0 * PI * freq * t).sin();
        }
        sample /= 4.0; // Normalize

        sample *= amplitude * 0.4; // Further reduced overall gain

        let scaled = (sample * i16::MAX as f32)
            .clamp(-i16::MAX as f32, i16::MAX as f32) as i16;
        samples.push(scaled);
    }

    samples
}

pub fn generate_crash_cymbal_sample(sample_rate: u32, sample_count: usize) -> Vec<i16> {
    let mut samples = Vec::with_capacity(sample_count);
    let mut rng = rand::rng();

    let decay_factor = 0.05; // Even faster initial decay
    let sustain_decay_factor = 0.005; // Even faster sustain decay
    let noise_freq_start = 8000.0;
    let noise_freq_end = 16000.0;

    for i in 0..sample_count {
        let t = i as f32 / sample_rate as f32;

        let amplitude = (-decay_factor * t).exp() + (-sustain_decay_factor * t).exp() * 0.2; // Further reduced sustain contribution

        let mut sample = 0.0;
        for _ in 0..10 { // Mix many high-frequency sine waves for noise
            let freq = rng.random_range(noise_freq_start..noise_freq_end);
            sample += (2.0 * PI * freq * t).sin();
        }
        sample /= 10.0; // Normalize

        sample *= amplitude * 0.5; // Further reduced overall gain

        let scaled = (sample * i16::MAX as f32)
            .clamp(-i16::MAX as f32, i16::MAX as f32) as i16;
        samples.push(scaled);
    }

    samples
}

pub fn generate_ride_cymbal_sample(sample_rate: u32, sample_count: usize) -> Vec<i16> {
    let mut samples = Vec::with_capacity(sample_count);

    let decay_factor = 0.02; // Even faster initial decay
    let sustain_decay_factor = 0.001; // Even faster sustain decay
    let fundamental_freq = 2000.0; // Fundamental frequency of the ride
    let harmonic_spread = 1.5; // How spread out the harmonics are

    for i in 0..sample_count {
        let t = i as f32 / sample_rate as f32;

        let amplitude = (-decay_factor * t).exp() + (-sustain_decay_factor * t).exp() * 0.3; // Further reduced sustain contribution

        let mut sample = 0.0;
        for j in 1..=8 { // Mix several harmonics
            let freq = fundamental_freq * (j as f32 * harmonic_spread);
            sample += (2.0 * PI * freq * t).sin() * (1.0 / j as f32);
        }
        sample /= 4.0; // Normalize

        sample *= amplitude * 0.5; // Further reduced overall gain

        let scaled = (sample * i16::MAX as f32)
            .clamp(-i16::MAX as f32, i16::MAX as f32) as i16;
        samples.push(scaled);
    }

    samples
}

pub fn generate_hihat_sample(sample_rate: u32, sample_count: usize) -> Vec<i16> {
    let mut samples = Vec::with_capacity(sample_count);
    let mut rng = rand::rng();

    let decay_factor = 0.25; // Even more significantly faster decay

    for i in 0..sample_count {
        let t = i as f32 / sample_rate as f32;

        let amplitude = (-decay_factor * t).exp();

        // Generate high-frequency noise for hi-hat
        let mut sample = 0.0;
        for _ in 0..6 { // Mix several sine waves at high frequencies
            let freq = rng.random_range(4000.0..10000.0);
            sample += (2.0 * PI * freq * t).sin();
        }
        sample /= 6.0; // Normalize

        sample *= amplitude * 0.4; // Further reduced overall gain

        let scaled = (sample * i16::MAX as f32)
            .clamp(-i16::MAX as f32, i16::MAX as f32) as i16;
        samples.push(scaled);
    }

    samples
}