//! 수중 음향 물리 (M1).
//!
//! - 소나 방정식: SNR = SL − TL − (NL − DI), SE = SNR − DT
//! - TL: 20log₁₀R + α(f)·R (Thorp 흡수 근사)
//! - NL: Knudsen C₀ 모델
//! - 수온약층: 깊이-수온 프로파일 → 음속 프로파일 (맥켄지 근사)
//!
//! M0: 핵심 함수 스텁 + 테스트 골격.

/// Thorp 흡수 계수 α(f) (dB/km).
///
/// Thorp (1965) 근사. `freq_khz`: 주파수 (kHz).
pub fn thorp_absorption_db_per_km(freq_khz: f32) -> f32 {
    let f2 = freq_khz * freq_khz;
    let a = 0.11 * f2 / (1.0 + 0.003 * f2);
    let b = 44.0 * f2 / (f2 + 4100.0);
    let c = 0.0027;
    let d = 0.000001 * f2 * f2 / (1.0 + 0.04 * f2);
    a + b + c + d
}

/// 전파 손실 TL (dB).
///
/// `range_m`: 거리 (m), `freq_khz`: 주파수 (kHz).
/// TL = 20log₁₀R + α(f)·R  (R km 단위)
pub fn transmission_loss_db(range_m: f32, freq_khz: f32) -> f32 {
    let r_km = range_m / 1000.0;
    let spherical = 20.0 * r_km.log10();
    let absorption = thorp_absorption_db_per_km(freq_khz) * r_km;
    spherical + absorption
}

/// 패시브 소나 신호 대 잡음비 (dB).
///
/// `sl`, `tl`, `nl`, `di`: dB.
pub fn snr_db(sl: f32, tl: f32, nl: f32, di: f32) -> f32 {
    sl - tl - (nl - di)
}

/// 신호 초과량 (dB).
pub fn signal_excess_db(snr: f32, dt: f32) -> f32 {
    snr - dt
}

/// 맥켄지 근사로 음속 (m/s) 계산.
///
/// `temp_c`: 수온 (°C), `salinity_psu`: 염분 (PSU), `depth_m`: 수심 (m).
pub fn mackenzie_sound_speed(temp_c: f32, salinity_psu: f32, depth_m: f32) -> f32 {
    let d = depth_m;
    let t = temp_c;
    let s = salinity_psu;
    1448.96 + 4.591 * t - 0.053 * t * t + 0.000237 * t * t * t
        + (1.340 - 0.010 * s) * (s - 35.0)
        + 0.0163 * d + 0.00017 * d * d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tl_increases_with_range() {
        let near = transmission_loss_db(1000.0, 1.0);
        let far = transmission_loss_db(10000.0, 1.0);
        assert!(far > near);
    }

    #[test]
    fn higher_freq_absorbs_more() {
        let low = transmission_loss_db(5000.0, 0.5);
        let high = transmission_loss_db(5000.0, 5.0);
        assert!(high > low);
    }

    #[test]
    fn snr_matches_equation() {
        // SNR = SL - TL - (NL - DI)
        let snr = snr_db(200.0, 120.0, 60.0, 10.0);
        assert!((snr - 30.0).abs() < 1e-6);
    }

    #[test]
    fn signal_excess_matches_equation() {
        let se = signal_excess_db(30.0, 5.0);
        assert!((se - 25.0).abs() < 1e-6);
    }

    #[test]
    fn sound_speed_increases_with_temperature() {
        let cold = mackenzie_sound_speed(2.0, 35.0, 500.0);
        let warm = mackenzie_sound_speed(20.0, 35.0, 500.0);
        assert!(warm > cold);
    }
}
