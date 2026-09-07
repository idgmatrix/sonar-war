//! 분석 이후의 오디오 출력 계층.
//!
//! 물리·수신기 신호를 변경하지 않고 최종 재생 경로의 범위만 제한한다.

/// 수신기 출력을 `(-1, 1)` 범위로 부드럽게 제한한다.
#[inline]
pub fn soft_limit(sample: f32) -> f32 {
    sample.tanh()
}

/// C등급 청취 보조: 2–120 Hz 성분을 240 Hz 반송파로 양측파대 변조한다.
/// 원시 센서/LOFAR 신호는 보존한다. 실제 수중 소리나 단측파대 주파수 이동이 아니다.
pub struct ListeningMonitor {
    low: f32,
    dc: f32,
    low_alpha: f32,
    dc_alpha: f32,
}

impl ListeningMonitor {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            low: 0.0,
            dc: 0.0,
            low_alpha: 1.0 - (-2.0 * std::f32::consts::PI * 120.0 / sample_rate).exp(),
            dc_alpha: 1.0 - (-2.0 * std::f32::consts::PI * 2.0 / sample_rate).exp(),
        }
    }
    pub fn sample(&mut self, input: f32, time: f64) -> f32 {
        self.low += self.low_alpha * (input - self.low);
        self.dc += self.dc_alpha * (input - self.dc);
        soft_limit(
            2.0 * (self.low - self.dc) * (2.0 * std::f64::consts::PI * 240.0 * time).cos() as f32,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiter_is_bounded_monotonic_and_symmetric() {
        let inputs = [-100.0, -2.0, -0.5, 0.0, 0.5, 2.0, 100.0];
        let outputs = inputs.map(soft_limit);
        assert!(outputs.iter().all(|sample| (-1.0..=1.0).contains(sample)));
        assert!(outputs.windows(2).all(|pair| pair[0] < pair[1]));
        assert!((outputs[0] + outputs[6]).abs() < 1e-6);
        assert!((outputs[2] + outputs[4]).abs() < 1e-6);
    }
}
