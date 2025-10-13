use rand::Rng;
use std::f32::consts::PI;

pub fn generate_piano_sample(sample_rate: u32, freq: f32, sample_count: usize) -> Vec<i16> {
    let mut samples = Vec::with_capacity(sample_count);
    let mut rng = rand::rng();

    let mut harmonics = vec![
        (1.0, 1.0),
        (2.01, 0.5),
        (3.02, 0.3),
        (4.98, 0.2),
        (6.1, 0.1),
    ];

    if freq < 400.0 {
        harmonics.push((0.5, 0.4));
        harmonics.push((7.03, 0.12));
        harmonics.push((8.97, 0.08));
        harmonics.push((10.09, 0.06));

        if freq < 150.0 {
            harmonics.push((0.25, 0.15));
            harmonics.push((1.5, 0.3));
        }
    }

    let phase_shifts: Vec<f32> = (0..harmonics.len())
        .map(|_| rng.random_range(0.0..2.0 * PI))
        .collect();

    let reference_low = 300.0;
    let reference_high = 20000.0;

    let frequency_scaling = if freq < reference_low {
        if freq < 80.0 {
            2.5 + 1.5 * (1.0 - freq / 80.0)
        } else if freq < 150.0 {
            1.8 + 0.7 * (1.0 - freq / 150.0)
        } else {
            1.3 + 0.5 * (1.0 - freq / reference_low)
        }
    } else if freq > reference_high {
        (-(freq - reference_high) * 0.0005).exp() * 0.6
    } else {
        1.0
    };

    let saturation_amount = if freq < 200.0 {
        0.6 + 0.4 * (1.0 - freq / 200.0)
    } else if freq < 500.0 {
        0.3 * (1.0 - (freq - 200.0) / 300.0)
    } else {
        0.0
    };

    for i in 0..sample_count {
        let t = i as f32 / sample_rate as f32;

        let attack_time = if freq < 150.0 {
            0.01 + 0.02 * (1.0 - freq / 150.0)
        } else {
            0.005 + 0.01 * (reference_high / freq).min(1.0)
        };

        let attack = if t < attack_time {
            t / attack_time
        } else {
            1.0
        };

        let decay_factor = if freq < reference_low {
            let base_decay = 0.5 + 0.5 * (freq / reference_low).powf(0.8);
            base_decay
        } else {
            2.0 + (freq / 440.0).min(1.0)
        };
        let decay = (-decay_factor * t).exp();
        let envelope = attack * decay;

        let mut sample = 0.0;
        for (idx, ((multiplier, amplitude), &phase)) in
            harmonics.iter().zip(phase_shifts.iter()).enumerate()
        {
            let harmonic_freq = freq * multiplier;

            let harmonic_attenuation = if harmonic_freq > reference_high {
                (-0.002 * (harmonic_freq - reference_high)).exp()
            } else {
                1.0
            };

            let harmonic_decay = (-t * (1.0 + idx as f32 * 0.4)).exp();

            let harmonic_boost = if freq < 200.0 {
                if *multiplier < 1.0 {
                    2.0
                } else if idx <= 3 {
                    1.6
                } else {
                    1.0
                }
            } else if freq < 300.0 && (idx <= 2 || *multiplier < 1.0) {
                1.3
            } else {
                1.0
            };

            sample += (2.0 * PI * harmonic_freq * t + phase).sin()
                * amplitude
                * harmonic_attenuation
                * harmonic_decay
                * harmonic_boost
                / 2.0;
        }

        // Hammer noise
        let noise_duration = 0.04;
        let noise_envelope = (-60.0 * t).exp();
        let noise = if t < noise_duration {
            let raw = rng.random_range(-1.0..1.0);
            let smooth = (raw + rng.random_range(-1.0..1.0)) * 0.5;
            smooth * 0.2 * noise_envelope
        } else {
            0.0
        };

        sample = (sample + noise) * envelope * frequency_scaling;

        if saturation_amount > 0.0 {
            let gain = 1.0 + saturation_amount * 2.0;
            sample = (sample * gain).tanh() / gain;

            let distortion_strength = saturation_amount * 0.1;
            let distorted = sample + (sample * sample * sample) * distortion_strength;
            sample = sample * (1.0 - saturation_amount * 0.3) + distorted * saturation_amount * 0.3;
        }

        let output_gain = if freq < 200.0 { 0.7 } else { 0.5 };

        let scaled = (sample * i16::MAX as f32 * output_gain)
            .clamp(-i16::MAX as f32, i16::MAX as f32) as i16;
        samples.push(scaled);
    }

    samples
}
