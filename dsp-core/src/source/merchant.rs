//! JOMOPANS-ECHO 상선 모집단 Source 모델.
//!
//! 근거와 기계 판독 원본: `data/acoustics/merchant-profiles.json`.
//! 모델 유효 범위는 20 Hz–20 kHz이며 개별 선박 고유 서명이 아니다.

use super::{SourceBand, SourceLevelReference, SourceLine, SourceSpectrum, TONAL_HARMONICS};

/// 현재 구현된 개별 상선 톤 유사체. 모집단 JOMOPANS 광대역 모델과 별도 선택한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum MerchantTonalOverlay {
    /// M/V OVERSEAS HARRIETTE, 140 RPM·16 kn, keel-aspect 측정점.
    OverseasHarriette140Rpm = 1,
}

impl MerchantTonalOverlay {
    pub fn from_code(code: u32) -> Option<Self> {
        match code {
            1 => Some(Self::OverseasHarriette140Rpm),
            _ => None,
        }
    }
}

/// Arveson–Vendittis Table III의 140 RPM 유의 톤 13개와 SSDG 고정 톤 2개.
/// 준위는 keel-aspect `dB re 1 µPa @ 1 m`; 배열 순서는 데이터 파일과 동일하다.
const OVERSEAS_HARRIETTE_140_LEVELS_DB: [f32; TONAL_HARMONICS as usize] = [
    174.0, 174.0, 175.0, 179.0, 185.0, 176.0, 175.0, 175.0, 172.0, 163.0, 172.0, 173.0, 170.0,
    179.0, 168.0,
];

/// 단일 측정 운항점의 톤 오버레이를 적용한다.
///
/// 이 함수는 다른 RPM으로 레벨을 외삽하지 않는다. 논문이 고조파 준위의 복잡하고
/// 비단조적인 속력 의존성을 보고했기 때문이다. 주파수도 측정 RPM(140)에서 고정된다.
pub fn apply_tonal_overlay(
    spectrum: &mut SourceSpectrum,
    overlay: MerchantTonalOverlay,
    shaft_rpm: f32,
    blade_count: u32,
) -> bool {
    match overlay {
        MerchantTonalOverlay::OverseasHarriette140Rpm => {
            if (shaft_rpm - 140.0).abs() > 0.5 || blade_count != 4 {
                return false;
            }
            let shaft_rate_hz = shaft_rpm / 60.0;
            // BR 1, FR 1, BR 2, BR 3=FR 2, BR 4, FR 3, BR 5,
            // BR 6=FR 4, BR 7, FR 5, BR 8, BR 9=FR 6, BR 10, SSDG 24/30 Hz.
            let shaft_multipliers = [
                4.0, 6.0, 8.0, 12.0, 16.0, 18.0, 20.0, 24.0, 28.0, 30.0, 32.0, 36.0, 40.0,
            ];
            spectrum.tonal_lines = std::array::from_fn(|index| SourceLine {
                frequency_hz: if index < shaft_multipliers.len() {
                    shaft_rate_hz * shaft_multipliers[index]
                } else if index == 13 {
                    24.0
                } else {
                    30.0
                },
                level_db_re_1upa_at_1m: OVERSEAS_HARRIETTE_140_LEVELS_DB[index],
                // Arveson–Vendittis Fig. 9/10: blade-rate 계열은 유효 수심 1.8 m의
                // 정확식, 나머지 저주파 기계 톤은 수심을 꾸며내지 않는 kd<<1 근사.
                level_reference: if matches!(index, 0 | 2 | 3 | 4 | 6 | 7 | 8 | 10 | 11 | 12) {
                    SourceLevelReference::KeelAspectPressureReleaseDipole {
                        effective_source_depth_m: 1.8,
                    }
                } else {
                    SourceLevelReference::KeelAspectLowFrequencyDipole
                },
            });
            // 논문은 blade-rate 변조를 관찰했지만 변조 깊이의 일반화 가능한 수치를
            // 제공하지 않는다. 임의의 포락선을 JOMOPANS 모집단 PSD에 곱하지 않는다.
            spectrum.modulation_rate_hz = 0.0;
            true
        }
    }
}

pub const DECIDECADE_CENTERS_HZ: [f32; 31] = [
    20.0, 25.2, 31.7, 40.0, 50.4, 63.5, 80.0, 100.8, 127.0, 160.0, 201.6, 254.0, 320.0, 403.2,
    508.0, 640.0, 806.3, 1015.9, 1280.0, 1612.7, 2031.9, 2560.0, 3225.4, 4063.7, 5120.0, 6450.8,
    8127.5, 10240.0, 12901.6, 16255.0, 20000.0,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum MerchantProfile {
    Bulker = 1,
    Containership = 2,
    VehicleCarrier = 3,
    Tanker = 4,
}

impl MerchantProfile {
    pub fn from_code(code: u32) -> Option<Self> {
        match code {
            1 => Some(Self::Bulker),
            2 => Some(Self::Containership),
            3 => Some(Self::VehicleCarrier),
            4 => Some(Self::Tanker),
            _ => None,
        }
    }

    fn reference_speed_kn(self) -> f32 {
        match self {
            Self::Bulker => 13.9,
            Self::Containership => 18.0,
            Self::VehicleCarrier => 15.8,
            Self::Tanker => 12.4,
        }
    }

    fn low_frequency_damping(self) -> f32 {
        match self {
            Self::Bulker | Self::Containership => 0.8,
            Self::VehicleCarrier | Self::Tanker => 1.0,
        }
    }
}

/// JOMOPANS-ECHO source spectrum level (dB re 1 µPa²/Hz @ 1 m).
pub fn spectrum_level_db(
    profile: MerchantProfile,
    frequency_hz: f32,
    speed_kn: f32,
    length_m: f32,
) -> f32 {
    const REFERENCE_LENGTH_M: f32 = 91.44;
    let frequency_hz = frequency_hz.clamp(20.0, 20_000.0);
    let reference_speed = profile.reference_speed_kn();
    let low = frequency_hz < 100.0;
    let (exponent, k, damping, numerator) = if low {
        (
            2.0_f32,
            208.0_f32,
            profile.low_frequency_damping(),
            600.0_f32,
        )
    } else {
        (0.0_f32, 191.0_f32, 3.0_f32, 480.0_f32)
    };
    let f1 = numerator / reference_speed;
    let frequency_power = 0.5 * (exponent + 2.0);
    k - 10.0 * (exponent + 2.0) * f1.log10() + 5.0 * exponent * frequency_hz.log10()
        - 10.0
            * ((1.0 - (frequency_hz / f1).powf(frequency_power)).powi(2) + damping.powi(2)).log10()
        + 60.0 * (speed_kn.max(0.1) / reference_speed).log10()
        + 20.0 * (length_m.max(1.0) / REFERENCE_LENGTH_M).log10()
}

/// 운항 상태에서 20 Hz–20 kHz 모집단 광대역 Source를 만든다.
/// 축·블레이드·기계류 톤은 별도 근거 모델이 연결될 때까지 생성하지 않는다.
pub fn source_spectrum(profile: MerchantProfile, speed_kn: f32, length_m: f32) -> SourceSpectrum {
    let broadband_bands = DECIDECADE_CENTERS_HZ
        .iter()
        .copied()
        .map(|center_hz| SourceBand {
            center_hz,
            spectrum_level_db_re_1upa2_per_hz_at_1m: spectrum_level_db(
                profile, center_hz, speed_kn, length_m,
            ),
        })
        .collect::<Vec<_>>();
    let broadband_power_upa2: f32 = broadband_bands
        .iter()
        .map(|band| {
            let psd = 10f32.powf(band.spectrum_level_db_re_1upa2_per_hz_at_1m / 10.0);
            // IEC decidecade 대역폭의 공학 근사: Δf ≈ 0.231 fc.
            psd * 0.231 * band.center_hz
        })
        .sum();
    SourceSpectrum {
        tonal_lines: [SourceLine {
            frequency_hz: 1.0,
            level_db_re_1upa_at_1m: f32::NEG_INFINITY,
            level_reference: SourceLevelReference::FreeField,
        }; TONAL_HARMONICS as usize],
        broadband_bands,
        // 대역 에너지를 전력 합산한 전체 광대역 RMS 준위.
        broadband_level_db_re_1upa_at_1m: 10.0 * broadband_power_upa2.log10(),
        modulation_rate_hz: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_bulker_calculator_anchors_match() {
        let anchors = [
            (63.095_734, 164.236_752),
            (100.0, 155.735_834),
            (1000.0, 137.757_591),
            (19_952.623, 111.516_992),
        ];
        for (frequency, expected) in anchors {
            let actual = spectrum_level_db(MerchantProfile::Bulker, frequency, 13.5, 211.0);
            assert!(
                (actual - expected).abs() < 0.002,
                "{frequency} Hz: {actual} != {expected}"
            );
        }
    }

    #[test]
    fn speed_and_length_raise_every_band() {
        let profile = MerchantProfile::Tanker;
        assert!(
            spectrum_level_db(profile, 1000.0, 15.0, 186.0)
                > spectrum_level_db(profile, 1000.0, 10.0, 186.0)
        );
        assert!(
            spectrum_level_db(profile, 1000.0, 12.4, 250.0)
                > spectrum_level_db(profile, 1000.0, 12.4, 150.0)
        );
    }

    #[test]
    fn profile_builds_validated_decidecade_bands_without_invented_tonals() {
        let spectrum = source_spectrum(MerchantProfile::Containership, 18.0, 294.0);
        assert_eq!(spectrum.broadband_bands.len(), 31);
        assert!(spectrum.broadband_level_db_re_1upa_at_1m.is_finite());
        assert!(spectrum
            .tonal_lines
            .iter()
            .all(|line| line.level_db_re_1upa_at_1m.is_infinite()));
        assert_eq!(spectrum.modulation_rate_hz, 0.0);
    }

    #[test]
    fn measured_140_rpm_overlay_reproduces_table_iii_lines() {
        let mut spectrum = source_spectrum(MerchantProfile::Bulker, 16.0, 172.9);
        assert!(apply_tonal_overlay(
            &mut spectrum,
            MerchantTonalOverlay::OverseasHarriette140Rpm,
            140.0,
            4,
        ));
        assert!((spectrum.tonal_lines[0].frequency_hz - 9.333_333).abs() < 0.001);
        assert_eq!(spectrum.tonal_lines[0].level_db_re_1upa_at_1m, 174.0);
        assert!((spectrum.tonal_lines[4].frequency_hz - 37.333_332).abs() < 0.001);
        assert_eq!(spectrum.tonal_lines[4].level_db_re_1upa_at_1m, 185.0);
        assert_eq!(spectrum.tonal_lines[13].frequency_hz, 24.0);
        assert_eq!(spectrum.tonal_lines[13].level_db_re_1upa_at_1m, 179.0);
        assert_eq!(spectrum.tonal_lines[14].frequency_hz, 30.0);
        assert_eq!(spectrum.tonal_lines[14].level_db_re_1upa_at_1m, 168.0);
        assert_eq!(
            spectrum.tonal_lines[0].level_reference,
            SourceLevelReference::KeelAspectPressureReleaseDipole {
                effective_source_depth_m: 1.8
            }
        );
        assert_eq!(
            spectrum.tonal_lines[1].level_reference,
            SourceLevelReference::KeelAspectLowFrequencyDipole
        );
        assert_eq!(spectrum.modulation_rate_hz, 0.0);
    }

    #[test]
    fn measured_overlay_refuses_unmeasured_operating_state() {
        let mut spectrum = source_spectrum(MerchantProfile::Bulker, 13.5, 211.0);
        let original = spectrum.clone();
        assert!(!apply_tonal_overlay(
            &mut spectrum,
            MerchantTonalOverlay::OverseasHarriette140Rpm,
            120.0,
            4,
        ));
        assert_eq!(spectrum, original);
    }
}
