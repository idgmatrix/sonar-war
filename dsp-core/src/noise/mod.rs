//! 해양 배경잡음 합성 (M2).
//!
//! - Knudsen C₀ 모델 (풍속 스케일)
//! - 함선/강우 옵션
//!
//! M0: 스텁.

/// Knudsen 해양 배경잡음 스펙트럼 준위 (dB re 1µPa²/Hz) 계산.
///
/// `freq_hz`: 주파수 (Hz), `wind_speed_ms`: 풍속 (m/s).
/// M2에서 완전한 Knudsen 스펙트럼으로 대체.
pub fn knudsen_noise_level_db(freq_hz: f32, wind_speed_ms: f32) -> f32 {
    // M0 스텁: 풍속 비례 광대역 레벨
    let _ = freq_hz;
    50.0 + 20.0 * (wind_speed_ms / 5.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_scales_with_wind() {
        let calm = knudsen_noise_level_db(100.0, 0.0);
        let storm = knudsen_noise_level_db(100.0, 15.0);
        assert!(storm > calm);
    }
}
