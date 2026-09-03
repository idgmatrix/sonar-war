//! 분석 이후의 오디오 출력 계층.
//!
//! 물리·수신기 신호를 변경하지 않고 최종 재생 경로의 범위만 제한한다.

/// 수신기 출력을 `(-1, 1)` 범위로 부드럽게 제한한다.
#[inline]
pub fn soft_limit(sample: f32) -> f32 {
    sample.tanh()
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
