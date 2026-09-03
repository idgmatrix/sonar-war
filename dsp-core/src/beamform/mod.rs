//! 빔포밍 (M2).
//!
//! 명세서 §3.2: 구형 배열 빔포밍 시간 지연
//!   τᵢ = pᵢ · û / c
//! 여기서 pᵢ = i번째 하이드로폰 위치 벡터, û = 목표 조향 단위 벡터, c = 음속.
//!
//! M2: 풀 지연-합 (Rust SIMD 최적화 여지).
//! M0: 스텁.

/// 하이드로폰 i의 빔포밍 시간 지연 (s).
///
/// `hydrophone`: 하이드로폰 위치 (x, depth, z) m,
/// `steer`: 조향 단위 벡터 (x, depth, z),
/// `sound_speed_ms`: 음속 (m/s).
pub fn beam_delay(hydrophone: [f32; 3], steer: [f32; 3], sound_speed_ms: f32) -> f32 {
    let dot = hydrophone[0] * steer[0] + hydrophone[1] * steer[1] + hydrophone[2] * steer[2];
    dot / sound_speed_ms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_is_zero_when_hydrophone_at_origin() {
        let d = beam_delay([0.0; 3], [1.0, 0.0, 0.0], 1500.0);
        assert!(d.abs() < 1e-6);
    }

    #[test]
    fn delay_positive_for_forward_hydrophone() {
        // +x 방향 조향, +x 쪽 하이드로폰 → 양의 지연
        let d = beam_delay([1.0, 0.0, 0.0], [1.0, 0.0, 0.0], 1500.0);
        assert!((d - 1.0 / 1500.0).abs() < 1e-6);
    }
}
