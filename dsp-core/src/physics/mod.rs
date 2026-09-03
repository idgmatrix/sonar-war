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

// ---------------------------------------------------------------------------
// 수온약층 (thermocline): 깊이-수온 프로파일 → 음속 프로파일 → TL 그리드/음영 구역
// ---------------------------------------------------------------------------

/// 깊이-수온 프로파일 (M1).
///
/// 표층(등온) → 약층(선형 감소) → 심층(등온) 3분할.
#[derive(Debug, Clone, Copy)]
pub struct TemperatureProfile {
    /// 표층 수온 (°C)
    pub surface_temp_c: f32,
    /// 약층 상단 깊이 (m)
    pub thermocline_top_m: f32,
    /// 약층 하단 깊이 (m)
    pub thermocline_bottom_m: f32,
    /// 심층 수온 (°C)
    pub deep_temp_c: f32,
    /// 염분 (PSU)
    pub salinity_psu: f32,
}

impl TemperatureProfile {
    /// 깊이 `depth_m`에서의 수온 (°C).
    pub fn temp_at(&self, depth_m: f32) -> f32 {
        let d = depth_m.max(0.0);
        if d <= self.thermocline_top_m {
            self.surface_temp_c
        } else if d >= self.thermocline_bottom_m {
            self.deep_temp_c
        } else {
            let t = (d - self.thermocline_top_m) / (self.thermocline_bottom_m - self.thermocline_top_m);
            self.surface_temp_c + t * (self.deep_temp_c - self.surface_temp_c)
        }
    }
}

/// 음속 프로파일 (깊이별 음속 샘플).
///
/// `TemperatureProfile`을 `mackenzie_sound_speed`로 샘플링해 생성.
#[derive(Debug, Clone)]
pub struct SoundSpeedProfile {
    /// 깊이 샘플 (m, 증가순, 0부터)
    pub depths: Vec<f32>,
    /// 각 깊이별 음속 (m/s)
    pub sound_speed: Vec<f32>,
    /// 프로파일 최대 깊이 (m)
    pub max_depth_m: f32,
}

impl SoundSpeedProfile {
    /// `TemperatureProfile`에서 `n`개 균등 깊이 샘플로 음속 프로파일 생성.
    pub fn from_temperature(profile: &TemperatureProfile, max_depth_m: f32, n: usize) -> Self {
        assert!(n >= 2, "최소 2개 샘플 필요");
        let mut depths = Vec::with_capacity(n);
        let mut sound_speed = Vec::with_capacity(n);
        for i in 0..n {
            let d = if n == 1 { 0.0 } else { max_depth_m * i as f32 / (n - 1) as f32 };
            let t = profile.temp_at(d);
            depths.push(d);
            sound_speed.push(mackenzie_sound_speed(t, profile.salinity_psu, d));
        }
        Self {
            depths,
            sound_speed,
            max_depth_m,
        }
    }

    /// 음속 최소값(= 음속최소층, 약층 바닥)의 깊이 (m).
    ///
    /// 수직 음선 굴절의 기준: 이 깊이 아래는 음영 구역.
    pub fn min_sound_speed_depth(&self) -> f32 {
        let mut best_i = 0;
        for i in 1..self.sound_speed.len() {
            if self.sound_speed[i] < self.sound_speed[best_i] {
                best_i = i;
            }
        }
        self.depths[best_i]
    }
}

/// 거리×깊이 TL 룩업 그리드 + 음영 구역 플래그 (M1).
///
/// 단일 주파수 기준. 음속최소층(약층 바닥) 아래에 있는 표적은 음영 구역으로
/// 판정하고 전파 손실에 벌점을 더한다.
#[derive(Debug, Clone)]
pub struct TlGrid {
    profile: SoundSpeedProfile,
    /// 주파수 (kHz)
    pub freq_khz: f32,
    /// 음영 구역 상단 깊이 (m) = 음속최소층 깊이
    pub shadow_zone_top_m: f32,
}

/// 음영 구역 진입 시 기본 전파 손실 벌점 (dB).
///
/// 직접 음선이 음속최소층에서 굴절되어 표적까지 도달하지 못해 생기는 손실.
/// 직접 경로가 차단되고 약한 굴절/반사 경로만 남으므로 큰 값(게임 근사,
/// 상수 시트 참조).
const SHADOW_BASE_DB: f32 = 40.0;
/// 음속최소층 아래 깊이당 추가 벌점 (dB/m).
const SHADOW_GRADIENT_DB_PER_M: f32 = 0.05;

impl TlGrid {
    /// 음속 프로파일 + 주파수로 그리드 생성.
    pub fn new(profile: SoundSpeedProfile, freq_khz: f32) -> Self {
        let shadow_zone_top_m = profile.min_sound_speed_depth();
        Self {
            profile,
            freq_khz,
            shadow_zone_top_m,
        }
    }

    /// 표적이 음영 구역에 들어가는지 판정.
    ///
    /// 음속최소층 아래(표적) + 그 위(음원)이면 직접 음선이 차단되어 음영.
    pub fn is_in_shadow_zone(&self, source_depth_m: f32, target_depth_m: f32) -> bool {
        target_depth_m > self.shadow_zone_top_m && source_depth_m <= self.shadow_zone_top_m
    }

    /// 음영 구역 벌점 (dB). 표적이 음속최소층 아래로 깊을수록 증가.
    pub fn shadow_penalty_db(&self, target_depth_m: f32) -> f32 {
        let excess = (target_depth_m - self.shadow_zone_top_m).max(0.0);
        SHADOW_BASE_DB + SHADOW_GRADIENT_DB_PER_M * excess
    }

    /// 거리×깊이 전파 손실 TL (dB).
    ///
    /// 기본 `20log₁₀R + α(f)·R` (Thorp) + 음영 구역 벌점(해당 시).
    pub fn transmission_loss(&self, range_m: f32, source_depth_m: f32, target_depth_m: f32) -> f32 {
        let base = transmission_loss_db(range_m, self.freq_khz);
        if self.is_in_shadow_zone(source_depth_m, target_depth_m) {
            base + self.shadow_penalty_db(target_depth_m)
        } else {
            base
        }
    }

    /// 음속 프로파일 참조.
    pub fn profile(&self) -> &SoundSpeedProfile {
        &self.profile
    }
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

    // --- M1 완료 기준: 거리/수심/주파수별 SNR·SE가 수식과 일치 ---

    #[test]
    fn snr_se_match_equations_over_grid() {
        // 음원(표층) → 표적(약층 위, 음영 아님) 파이프라인을 수식과 대조.
        let sl = 165.0;
        let di = 12.0;
        let dt = 5.0;
        let nl = 60.0;
        let src_depth = 20.0;

        let profile = SoundSpeedProfile::from_temperature(
            &TemperatureProfile {
                surface_temp_c: 22.0,
                thermocline_top_m: 50.0,
                thermocline_bottom_m: 300.0,
                deep_temp_c: 4.0,
                salinity_psu: 35.0,
            },
            1000.0,
            101,
        );

        for &freq in &[0.5f32, 1.0, 2.0, 5.0] {
            let grid = TlGrid::new(profile.clone(), freq);
            for &range in &[500.0f32, 2000.0, 5000.0, 10000.0] {
                for &tgt_depth in &[20.0f32, 100.0, 250.0] {
                    let tl = grid.transmission_loss(range, src_depth, tgt_depth);
                    let snr = snr_db(sl, tl, nl, di);
                    let se = signal_excess_db(snr, dt);
                    // 수식 직접 대조
                    let expect_tl = transmission_loss_db(range, freq);
                    let expect_se = sl - expect_tl - (nl - di) - dt;
                    assert!((tl - expect_tl).abs() < 1e-4, "TL mismatch f={freq} R={range} d={tgt_depth}");
                    assert!((se - expect_se).abs() < 1e-4, "SE mismatch f={freq} R={range} d={tgt_depth}");
                }
            }
        }
    }

    // --- M1 완료 기준: 약층 아래 표적이 음영 구역에 들어감에 따라 SE가 임계 이하로 떨어짐 ---

    #[test]
    fn below_thermocline_target_drops_below_threshold_in_shadow() {
        let profile = SoundSpeedProfile::from_temperature(
            &TemperatureProfile {
                surface_temp_c: 22.0,
                thermocline_top_m: 50.0,
                thermocline_bottom_m: 300.0,
                deep_temp_c: 4.0,
                salinity_psu: 35.0,
            },
            1000.0,
            101,
        );
        // 음속최소층은 심층(300m 이하, 4°C)에 있어야 함
        let min_depth = profile.min_sound_speed_depth();
        assert!(min_depth >= 300.0 - 1e-3, "음속최소층이 약층 바닥 아래여야 함: {min_depth}");

        let grid = TlGrid::new(profile, 1.0);
        // 조용한 표적(저 SL) + 중거리: 음영 아님엔 검출 가능, 음영엔 이탈.
        let (sl, di, dt, nl) = (120.0f32, 10.0, 5.0, 60.0);
        let src_depth = 20.0; // 표층 음원
        let range = 10000.0;
        let threshold = 5.0; // SE 임계 (dB)

        // 약층 위 표적(200m): 음영 아님 → SE 임계 초과
        let se_above = signal_excess_db(snr_db(sl, grid.transmission_loss(range, src_depth, 200.0), nl, di), dt);
        assert!(se_above > threshold, "약층 위 표적은 임계 초과여야 함: {se_above}");

        // 약층 아래 표적(400m): 음영 진입 → SE 임계 이하
        assert!(grid.is_in_shadow_zone(src_depth, 400.0), "400m 표적은 음영 구역");
        let se_below = signal_excess_db(snr_db(sl, grid.transmission_loss(range, src_depth, 400.0), nl, di), dt);
        assert!(se_below < threshold, "음영 표적은 임계 이하로 떨어져야 함: {se_below}");

        // 음영 진입으로 SE가 실제로 감소
        assert!(se_below < se_above);
    }

    #[test]
    fn shadow_zone_flag_and_penalty_monotonic() {
        let profile = SoundSpeedProfile::from_temperature(
            &TemperatureProfile {
                surface_temp_c: 22.0,
                thermocline_top_m: 50.0,
                thermocline_bottom_m: 300.0,
                deep_temp_c: 4.0,
                salinity_psu: 35.0,
            },
            1000.0,
            101,
        );
        let grid = TlGrid::new(profile, 1.0);
        // 음원도 음영층 아래면 직접 음선 차단이 아님(표층-심층 간 굴절 문제 아님)
        assert!(!grid.is_in_shadow_zone(400.0, 500.0));
        // 벌점이 깊이에 대해 단조 증가
        let p1 = grid.shadow_penalty_db(350.0);
        let p2 = grid.shadow_penalty_db(500.0);
        assert!(p2 > p1);
    }
}
