#[allow(dead_code)]
pub struct Limiter {
    sample_rate: f32,
    threshold_db: f32,
    threshold: f32,
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
        assert!(threshold_db <= 0.0, "Threshold should be <= 0 dBFS");
        assert!(release_time_ms > 0.0, "Release time must be positive");
        assert!(lookahead_ms > 0.0, "Lookahead time must be positive");

        let threshold = 10f32.powf(threshold_db / 20.0);
        let release_coef = (-1.0 / (release_time_ms / 1000.0 * sample_rate)).exp();
        let lookahead_coef = (-1.0 / (lookahead_ms / 1000.0 * sample_rate)).exp();

        Limiter {
            sample_rate,
            threshold_db,
            threshold,
            release_coef,
            lookahead_coef,
            peak_envelope: 0.0,
            smoothed_gain: 1.0,
        }
    }

    pub fn process(&mut self, buffer: &mut [f32]) {
        for sample in buffer.iter_mut() {
            let abs_sample = sample.abs();

            // Step 2: Update peak envelope with lookahead smoothing
            self.peak_envelope *= self.lookahead_coef;
            if abs_sample > self.peak_envelope {
                self.peak_envelope = abs_sample;
            }

            // Step 3: Calculate instantaneous target gain
            let inst_gain = if self.peak_envelope <= self.threshold {
                1.0
            } else {
                self.threshold / self.peak_envelope
            };

            // Step 4: Smooth gain change
            if inst_gain < self.smoothed_gain {
                // Fast attack
                self.smoothed_gain = inst_gain;
            } else {
                // Smooth release
                self.smoothed_gain =
                    self.release_coef * self.smoothed_gain + (1.0 - self.release_coef) * inst_gain;
            }

            // Step 5: Apply gain to original sample
            *sample *= self.smoothed_gain;
        }
    }
}
