//! 엔티티 소음 합성 (M2).
//!
//! - 토널 (LOFAR 라인)
//! - 캐비테이션 광대역
//! - DEMON 변조 (캐비테이션 포락선을 bladeRate로 AM)
//! - 도플러 시프트 (상대 속도 기반)
//!
//! M0: 스텁.

/// 엔티티 음원의 광대역 소음 레벨 (dB).
/// M2: RPM/캐비테이션/선체에 따른 완전한 모델.
pub fn broadband_level_db(rpm: f32, cavitation: f32) -> f32 {
    let _ = rpm;
    120.0 + 20.0 * cavitation
}

/// 도플러 주파수 시프트 계수.
///
/// `rel_vel_ms`: 수신자 방향으로의 상대 속도 (m/s, 접근 시 +),
/// `sound_speed_ms`: 음속 (m/s).
/// 도플러 계수 = 1 + v/c.
pub fn doppler_factor(rel_vel_ms: f32, sound_speed_ms: f32) -> f32 {
    1.0 + rel_vel_ms / sound_speed_ms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approaching_target_raises_frequency() {
        let f = doppler_factor(10.0, 1500.0);
        assert!(f > 1.0);
    }

    #[test]
    fn receding_target_lowers_frequency() {
        let f = doppler_factor(-10.0, 1500.0);
        assert!(f < 1.0);
    }

    #[test]
    fn cavitation_raises_broadband() {
        assert!(broadband_level_db(90.0, 0.8) > broadband_level_db(90.0, 0.1));
    }
}
