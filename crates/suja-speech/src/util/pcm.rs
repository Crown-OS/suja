pub fn peak_level(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |peak, s| peak.max(s.abs()))
}

pub fn rms_level(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let energy: f32 = samples.iter().map(|s| s * s).sum();
    (energy / samples.len() as f32).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_of_silence_are_zero() {
        assert_eq!(peak_level(&[0.0; 8]), 0.0);
        assert_eq!(rms_level(&[]), 0.0);
    }

    #[test]
    fn peak_tracks_the_loudest_sample() {
        assert_eq!(peak_level(&[0.1, -0.7, 0.3]), 0.7);
    }
}
