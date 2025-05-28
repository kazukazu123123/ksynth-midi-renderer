pub struct Limiter {
    threshold: f32,
    epsilon: f32,
    release_coef: f32,
    lookahead_coef: f32,
    peak_envelope: f32,
    smoothed_gain: f32,
}

impl Limiter {
    pub fn new(
        sample_rate: f32,
        threshold_db: f32,
        release_time_ms: f32,
        lookahead_ms: f32,
    ) -> Self {
        let threshold = 10.0f32.powf(threshold_db / 20.0); // dBFS -> Linear
        let release_coef = (-1.0 / (release_time_ms / 1000.0 * sample_rate)).exp();
        let lookahead_coef = (-1.0 / (lookahead_ms / 1000.0 * sample_rate)).exp();

        Limiter {
            threshold,
            epsilon: 1e-9,
            release_coef,
            lookahead_coef,
            peak_envelope: 0.0,
            smoothed_gain: 1.0,
        }
    }

    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        let mut output = Vec::with_capacity(input.len());

        for &sample in input {
            let abs_sample = sample.abs();

            // 1. Update peak envelope with lookahead smoothing
            self.peak_envelope *= self.lookahead_coef;
            if abs_sample > self.peak_envelope {
                self.peak_envelope = abs_sample;
            }

            // 2. Compute instantaneous gain
            let inst_gain = if self.peak_envelope <= self.threshold {
                1.0
            } else {
                self.threshold / self.peak_envelope.max(self.epsilon)
            };

            // 3. Apply gain smoothing
            if inst_gain < self.smoothed_gain {
                self.smoothed_gain = inst_gain; // Fast attack
            } else {
                // Smooth release
                self.smoothed_gain =
                    self.release_coef * self.smoothed_gain + (1.0 - self.release_coef) * inst_gain;
            }

            // 4. Apply gain to sample
            output.push(sample * self.smoothed_gain);
        }

        output
    }
}
