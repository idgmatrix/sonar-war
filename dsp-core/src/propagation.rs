//! Source 스펙트럼에 거리·도플러·배열 도달 지연을 적용하는 전파 계층.
//!
//! 입력은 `dB re 1 µPa @ 1 m`, 출력은 수신점의 `dB re 1 µPa`다.
//! full-scale 변환과 빔포밍은 Receiver 계층의 책임이다.

use crate::beamform::beam_delay;
use crate::physics::transmission_loss_db;
use crate::source::{
    SourceLevelReference, SourceLine, SourceSpectrum, SourceVoice, TONAL_HARMONICS,
};

/// 캐비테이션 광대역 TL을 평가하는 기준 주파수 (kHz).
pub const BROADBAND_REFERENCE_KHZ: f32 = 1.0;

/// 상대 속도에 따른 도플러 주파수 계수. 수신기 방향 접근이 양수다.
#[inline]
pub fn doppler_factor(relative_velocity_ms: f32, sound_speed_ms: f32) -> f32 {
    1.0 + relative_velocity_ms / sound_speed_ms
}

/// keel 방향 측정 준위를 현재 고각으로 옮기는 자유수면 압력 해제 보정(dB).
///
/// `depression_sine=0`은 수평, `1`은 keel 방향이다. 정확식은
/// `|sin(k d sinθ)| / |sin(k d)|`; 저주파 극한에서는 `|sinθ|`가 된다.
pub fn source_level_reference_adjustment_db(
    reference: SourceLevelReference,
    frequency_hz: f32,
    depression_sine: f32,
    sound_speed_ms: f32,
) -> f32 {
    let depression_sine = depression_sine.clamp(0.0, 1.0);
    let pressure_ratio = match reference {
        SourceLevelReference::FreeField => return 0.0,
        SourceLevelReference::KeelAspectPressureReleaseDipole {
            effective_source_depth_m,
        } => {
            let kd = 2.0 * std::f32::consts::PI * frequency_hz * effective_source_depth_m
                / sound_speed_ms.max(f32::MIN_POSITIVE);
            let keel = kd.sin().abs();
            if keel <= f32::MIN_POSITIVE {
                depression_sine
            } else {
                (kd * depression_sine).sin().abs() / keel
            }
        }
        SourceLevelReference::KeelAspectLowFrequencyDipole => depression_sine,
    };
    if pressure_ratio == 0.0 {
        f32::NEG_INFINITY
    } else {
        20.0 * pressure_ratio.log10()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PropagationGeometry {
    pub bearing_deg: f32,
    pub range_m: f32,
    pub source_depth_m: f32,
    pub receiver_depth_m: f32,
    /// 수신기 방향 속도. 접근이 양수다.
    pub relative_velocity_ms: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReceivedLine {
    pub frequency_hz: f32,
    pub level_db_re_1upa: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReceivedBand {
    pub center_hz: f32,
    pub spectrum_level_db_re_1upa2_per_hz: f32,
}

/// 단일 표적이 수신 배열 위치에 만든 스펙트럼과 공간 지연.
#[derive(Debug, Clone, PartialEq)]
pub struct PropagatedSpectrum {
    pub tonal_lines: [ReceivedLine; TONAL_HARMONICS as usize],
    pub broadband_bands: Vec<ReceivedBand>,
    pub broadband_level_db_re_1upa: f32,
    pub modulation_rate_hz: f32,
    /// 하이드로폰별 인과적 읽기 지연(s). 모두 0 이상이다.
    pub hydrophone_delays_s: Vec<f32>,
    /// 수신기에서 소스를 향하는 단위 벡터. x=전방, y=하향, z=우현.
    pub arrival_direction: [f32; 3],
}

/// 한 오디오 샘플 시각의 배열 입력. 값은 아직 수신기 FS로 정규화되지 않은 µPa다.
#[derive(Debug, Clone, PartialEq)]
pub struct HydrophoneFrame {
    pub pressure_upa: Vec<f32>,
}

impl HydrophoneFrame {
    pub fn new(hydrophone_count: usize) -> Self {
        Self {
            pressure_upa: vec![0.0; hydrophone_count],
        }
    }

    #[inline]
    pub fn clear(&mut self) {
        self.pressure_upa.fill(0.0);
    }
}

/// 한 Source의 상태형 샘플을 배열 위치까지 전파하는 고정 파라미터 프로세서.
#[derive(Debug, Clone)]
pub struct PropagationProcessor {
    tonal_linear_loss: [f32; TONAL_HARMONICS as usize],
    broadband_linear_loss: f32,
    broadband_band_linear_loss: Vec<f32>,
    broadband_history_upa: Vec<f32>,
    broadband_history_position: usize,
    doppler_factor: f32,
    hydrophone_delays_s: Vec<f32>,
}

impl PropagationProcessor {
    pub fn new(
        source: &SourceSpectrum,
        propagated: &PropagatedSpectrum,
        sample_rate: f32,
        history_samples: usize,
    ) -> Self {
        let tonal_linear_loss = std::array::from_fn(|index| {
            let source_level = source.tonal_lines[index].level_db_re_1upa_at_1m;
            let received_level = propagated.tonal_lines[index].level_db_re_1upa;
            if source_level.is_finite() && received_level.is_finite() {
                10f32.powf((received_level - source_level) / 20.0)
            } else {
                0.0
            }
        });
        let broadband_linear_loss = 10f32.powf(
            (propagated.broadband_level_db_re_1upa - source.broadband_level_db_re_1upa_at_1m)
                / 20.0,
        );
        let nyquist_guard = sample_rate * 0.45;
        let broadband_band_linear_loss = source
            .broadband_bands
            .iter()
            .zip(&propagated.broadband_bands)
            .filter(|(source, _)| source.center_hz < nyquist_guard)
            .map(|(source, received)| {
                10f32.powf(
                    (received.spectrum_level_db_re_1upa2_per_hz
                        - source.spectrum_level_db_re_1upa2_per_hz_at_1m)
                        / 20.0,
                )
            })
            .collect();
        let doppler_factor = propagated.tonal_lines[0].frequency_hz
            / source.tonal_lines[0].frequency_hz.max(f32::MIN_POSITIVE);
        Self {
            tonal_linear_loss,
            broadband_linear_loss,
            broadband_band_linear_loss,
            broadband_history_upa: vec![0.0; history_samples.max(2)],
            broadband_history_position: 0,
            doppler_factor,
            hydrophone_delays_s: propagated.hydrophone_delays_s.clone(),
        }
    }

    /// Source의 현재 대역 압력에 대역별 TL을 적용해 전파 계층 지연 버퍼에 기록한다.
    #[inline]
    pub fn advance_broadband(&mut self, source: &SourceVoice) {
        if self.broadband_band_linear_loss.is_empty() {
            return;
        }
        let propagated = source
            .broadband_band_pressures_1m_upa()
            .zip(&self.broadband_band_linear_loss)
            .map(|(pressure, loss)| pressure * loss)
            .sum();
        self.broadband_history_upa[self.broadband_history_position] = propagated;
        self.broadband_history_position =
            (self.broadband_history_position + 1) % self.broadband_history_upa.len();
    }

    fn read_broadband(&self, offset_samples: f32) -> f32 {
        let capacity = self.broadband_history_upa.len();
        let offset = offset_samples.max(0.0).min(capacity as f32 - 2.0);
        let whole = offset.floor() as usize;
        let fraction = offset - whole as f32;
        let newest = if self.broadband_history_position == 0 {
            capacity - 1
        } else {
            self.broadband_history_position - 1
        };
        let first = newest.wrapping_sub(whole) % capacity;
        let second = newest.wrapping_sub(whole + 1) % capacity;
        self.broadband_history_upa[first] * (1.0 - fraction)
            + self.broadband_history_upa[second] * fraction
    }

    /// Source 샘플을 전파해 재사용 `HydrophoneFrame`에 누적한다.
    #[inline]
    pub fn render_into(
        &self,
        source: &SourceVoice,
        receiver_time_s: f64,
        sample_rate: f32,
        frame: &mut HydrophoneFrame,
    ) {
        debug_assert_eq!(frame.pressure_upa.len(), self.hydrophone_delays_s.len());
        for (hydrophone, &delay_s) in self.hydrophone_delays_s.iter().enumerate() {
            let source_time_s = (receiver_time_s - delay_s as f64) * self.doppler_factor as f64;
            let sample = source.sample_at(source_time_s, delay_s * sample_rate);
            let tonal = sample
                .tonal_pressure_1m_upa
                .iter()
                .zip(self.tonal_linear_loss)
                .map(|(pressure, loss)| pressure * loss)
                .sum::<f32>();
            let broadband = if self.broadband_band_linear_loss.is_empty() {
                sample.broadband_pressure_1m_upa * self.broadband_linear_loss
            } else {
                self.read_broadband(delay_s * sample_rate)
            };
            frame.pressure_upa[hydrophone] += tonal + broadband;
        }
    }
}

pub fn propagate(
    source: &SourceSpectrum,
    geometry: PropagationGeometry,
    hydrophones: &[[f32; 3]],
    sound_speed_ms: f32,
) -> PropagatedSpectrum {
    let range_m = geometry.range_m.max(1.0);
    let azimuth = geometry.bearing_deg.to_radians();
    let vertical_offset_m = geometry.source_depth_m - geometry.receiver_depth_m;
    let horizontal = (range_m * range_m - vertical_offset_m * vertical_offset_m)
        .max(0.0)
        .sqrt();
    let arrival_direction = [
        horizontal * azimuth.cos() / range_m,
        vertical_offset_m / range_m,
        horizontal * azimuth.sin() / range_m,
    ];

    // 실시간 스트리밍은 미래 샘플을 읽을 수 없으므로 모든 지연에 동일한 pmax를 더한다.
    let pmax = hydrophones
        .iter()
        .map(|position| beam_delay(*position, arrival_direction, sound_speed_ms))
        .fold(0.0f32, f32::max);
    let hydrophone_delays_s = hydrophones
        .iter()
        .map(|position| (pmax - beam_delay(*position, arrival_direction, sound_speed_ms)).max(0.0))
        .collect();

    let doppler = doppler_factor(geometry.relative_velocity_ms, sound_speed_ms);
    let tonal_lines = std::array::from_fn(|index| {
        let SourceLine {
            frequency_hz,
            level_db_re_1upa_at_1m,
            level_reference,
        } = source.tonal_lines[index];
        let shifted_frequency_hz = frequency_hz * doppler;
        let depression_sine = (vertical_offset_m / range_m).abs().clamp(0.0, 1.0);
        let reference_adjustment_db = source_level_reference_adjustment_db(
            level_reference,
            frequency_hz,
            depression_sine,
            sound_speed_ms,
        );
        ReceivedLine {
            frequency_hz: shifted_frequency_hz,
            level_db_re_1upa: level_db_re_1upa_at_1m + reference_adjustment_db
                - transmission_loss_db(range_m, shifted_frequency_hz / 1000.0),
        }
    });
    let broadband_bands = source
        .broadband_bands
        .iter()
        .map(|band| ReceivedBand {
            center_hz: band.center_hz,
            spectrum_level_db_re_1upa2_per_hz: band.spectrum_level_db_re_1upa2_per_hz_at_1m
                - transmission_loss_db(range_m, band.center_hz / 1000.0),
        })
        .collect::<Vec<_>>();
    let broadband_level_db_re_1upa = if broadband_bands.is_empty() {
        source.broadband_level_db_re_1upa_at_1m
            - transmission_loss_db(range_m, BROADBAND_REFERENCE_KHZ)
    } else {
        let pressure_power_upa2: f32 = broadband_bands
            .iter()
            .map(|band| {
                10f32.powf(band.spectrum_level_db_re_1upa2_per_hz / 10.0) * 0.231 * band.center_hz
            })
            .sum();
        10.0 * pressure_power_upa2.log10()
    };

    PropagatedSpectrum {
        tonal_lines,
        broadband_bands,
        broadband_level_db_re_1upa,
        modulation_rate_hz: source.modulation_rate_hz * doppler,
        hydrophone_delays_s,
        arrival_direction,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offline::{power_spectrum, strongest_peak, tone_rms_level_db};
    use crate::source::merchant::{
        self, apply_tonal_overlay, MerchantProfile, MerchantTonalOverlay,
    };
    use crate::source::source_spectrum;

    fn measured_psd_db(samples: &[f32], sample_rate: f32, frequency_hz: f32) -> f32 {
        const SEGMENT: usize = 8192;
        let mut psd_sum = 0.0f64;
        let mut segments = 0usize;
        let (chunks, _) = samples.as_chunks::<SEGMENT>();
        for chunk in chunks {
            let mut re = 0.0f64;
            let mut im = 0.0f64;
            let mut window_energy = 0.0f64;
            for (index, &sample) in chunk.iter().enumerate() {
                let window = 0.5
                    - 0.5
                        * (2.0 * std::f64::consts::PI * index as f64 / (SEGMENT - 1) as f64).cos();
                let phase = 2.0 * std::f64::consts::PI * frequency_hz as f64 * index as f64
                    / sample_rate as f64;
                let value = sample as f64 * window;
                re += value * phase.cos();
                im -= value * phase.sin();
                window_energy += window * window;
            }
            psd_sum += 2.0 * (re * re + im * im) / (sample_rate as f64 * window_energy);
            segments += 1;
        }
        10.0 * (psd_sum / segments as f64).max(f64::MIN_POSITIVE).log10() as f32
    }

    #[test]
    fn range_changes_propagation_but_not_source() {
        let source = source_spectrum(120.0, 5, 150.0, 0.4);
        let original = source.clone();
        let hydrophones = [[-4.2, 0.0, 0.0], [4.2, 0.0, 0.0]];
        let geometry = PropagationGeometry {
            bearing_deg: 0.0,
            range_m: 1000.0,
            source_depth_m: 30.0,
            receiver_depth_m: 100.0,
            relative_velocity_ms: 0.0,
        };
        let near = propagate(&source, geometry, &hydrophones, 1500.0);
        let far = propagate(
            &source,
            PropagationGeometry {
                range_m: 8000.0,
                ..geometry
            },
            &hydrophones,
            1500.0,
        );
        assert_eq!(source, original);
        assert_eq!(
            near.tonal_lines[0].frequency_hz,
            far.tonal_lines[0].frequency_hz
        );
        assert!(near.tonal_lines[0].level_db_re_1upa > far.tonal_lines[0].level_db_re_1upa);
        assert!(
            near.arrival_direction[1] < 0.0,
            "더 얕은 소스는 위쪽 도달 방향"
        );
    }

    #[test]
    fn doppler_changes_propagated_frequency_only() {
        let source = source_spectrum(120.0, 5, 150.0, 0.4);
        let original = source.clone();
        let approaching = propagate(
            &source,
            PropagationGeometry {
                bearing_deg: 0.0,
                range_m: 1000.0,
                source_depth_m: 30.0,
                receiver_depth_m: 100.0,
                relative_velocity_ms: 20.0,
            },
            &[],
            1500.0,
        );
        assert_eq!(source, original);
        assert!(approaching.tonal_lines[0].frequency_hz > source.tonal_lines[0].frequency_hz);
    }

    #[test]
    fn doppler_sign_matches_approach_convention() {
        assert!(doppler_factor(10.0, 1500.0) > 1.0);
        assert!(doppler_factor(-10.0, 1500.0) < 1.0);
    }

    #[test]
    fn pressure_release_dipole_preserves_keel_reference_and_nulls_horizontal() {
        let exact = SourceLevelReference::KeelAspectPressureReleaseDipole {
            effective_source_depth_m: 1.8,
        };
        assert_eq!(
            source_level_reference_adjustment_db(exact, 93.333_336, 1.0, 1500.0),
            0.0
        );
        assert!(source_level_reference_adjustment_db(exact, 93.333_336, 0.0, 1500.0).is_infinite());
        assert_eq!(
            source_level_reference_adjustment_db(
                SourceLevelReference::KeelAspectLowFrequencyDipole,
                24.0,
                1.0,
                1500.0,
            ),
            0.0
        );
    }

    #[test]
    fn exact_pressure_release_factor_is_frequency_dependent() {
        let reference = SourceLevelReference::KeelAspectPressureReleaseDipole {
            effective_source_depth_m: 1.8,
        };
        let low = source_level_reference_adjustment_db(reference, 9.333_333, 0.5, 1500.0);
        let high = source_level_reference_adjustment_db(reference, 93.333_33, 0.5, 1500.0);
        let low_frequency_limit_db = 20.0 * 0.5f32.log10();
        assert!((low - low_frequency_limit_db).abs() < 0.01, "low={low}");
        assert!(high > low + 0.35, "low={low} high={high}");
    }

    #[test]
    fn measured_merchant_directionality_is_applied_only_in_propagation() {
        let mut source = merchant::source_spectrum(MerchantProfile::Bulker, 16.0, 172.9);
        assert!(apply_tonal_overlay(
            &mut source,
            MerchantTonalOverlay::OverseasHarriette140Rpm,
            140.0,
            4,
        ));
        let original = source.clone();
        let propagated = propagate(
            &source,
            PropagationGeometry {
                bearing_deg: 0.0,
                range_m: 1000.0,
                source_depth_m: 500.0,
                receiver_depth_m: 0.0,
                relative_velocity_ms: 0.0,
            },
            &[],
            1500.0,
        );
        assert_eq!(
            source, original,
            "Propagation은 Source 기준값을 변경하지 않는다"
        );
        let tl = transmission_loss_db(1000.0, source.tonal_lines[0].frequency_hz / 1000.0);
        let correction = propagated.tonal_lines[0].level_db_re_1upa
            - (source.tonal_lines[0].level_db_re_1upa_at_1m - tl);
        let expected = source_level_reference_adjustment_db(
            source.tonal_lines[0].level_reference,
            source.tonal_lines[0].frequency_hz,
            0.5,
            1500.0,
        );
        assert!((correction - expected).abs() < 0.002);
    }

    #[test]
    fn merchant_bands_receive_frequency_dependent_loss() {
        let source = merchant::source_spectrum(MerchantProfile::Bulker, 13.5, 211.0);
        let propagated = propagate(
            &source,
            PropagationGeometry {
                bearing_deg: 0.0,
                range_m: 50_000.0,
                source_depth_m: 6.0,
                receiver_depth_m: 100.0,
                relative_velocity_ms: 0.0,
            },
            &[],
            1500.0,
        );
        assert_eq!(
            source.broadband_bands.len(),
            propagated.broadband_bands.len()
        );
        for (source_band, received_band) in source
            .broadband_bands
            .iter()
            .zip(&propagated.broadband_bands)
        {
            let expected_loss = transmission_loss_db(50_000.0, source_band.center_hz / 1000.0);
            let actual_loss = source_band.spectrum_level_db_re_1upa2_per_hz_at_1m
                - received_band.spectrum_level_db_re_1upa2_per_hz;
            assert!((actual_loss - expected_loss).abs() < 0.002);
        }
        let low_loss = source.broadband_bands[7].spectrum_level_db_re_1upa2_per_hz_at_1m
            - propagated.broadband_bands[7].spectrum_level_db_re_1upa2_per_hz;
        let high_loss = source.broadband_bands[27].spectrum_level_db_re_1upa2_per_hz_at_1m
            - propagated.broadband_bands[27].spectrum_level_db_re_1upa2_per_hz;
        assert!(
            high_loss > low_loss + 40.0,
            "low={low_loss} high={high_loss}"
        );
    }

    #[test]
    fn measured_merchant_tones_match_source_and_1km_propagation_golden() {
        const SAMPLE_RATE: f32 = 1024.0;
        const DURATION_S: usize = 8;
        const RANGE_M: f32 = 1000.0;
        const PEAK_SEARCH_HALF_WIDTH_HZ: f64 = 0.75;

        let golden =
            include_str!("../../data/acoustics/golden/overseas_harriette_140rpm_tones_1km.csv")
                .lines()
                .filter(|line| {
                    !line.is_empty() && !line.starts_with('#') && !line.starts_with("frequency_hz")
                })
                .map(|line| {
                    let fields = line
                        .split(',')
                        .map(|field| field.parse::<f64>().expect("상선 톤 골든 숫자"))
                        .collect::<Vec<_>>();
                    assert_eq!(fields.len(), 6);
                    fields
                })
                .collect::<Vec<_>>();
        assert_eq!(golden.len(), 15);

        let mut spectrum = merchant::source_spectrum(MerchantProfile::Bulker, 16.0, 172.9);
        assert!(apply_tonal_overlay(
            &mut spectrum,
            MerchantTonalOverlay::OverseasHarriette140Rpm,
            140.0,
            4,
        ));
        // 측정 톤만 검증한다. JOMOPANS 광대역과 수신기 잡음은 별도 골든이 담당한다.
        spectrum.broadband_bands.clear();
        spectrum.broadband_level_db_re_1upa_at_1m = -300.0;

        let propagated = propagate(
            &spectrum,
            PropagationGeometry {
                bearing_deg: 0.0,
                range_m: RANGE_M,
                source_depth_m: 6.0,
                // Table III는 keel-aspect 준위이므로 1 km 수직 경로에서 검증한다.
                receiver_depth_m: 1006.0,
                relative_velocity_ms: 0.0,
            },
            &[[0.0, 0.0, 0.0]],
            1500.0,
        );
        let mut voice = SourceVoice::new(spectrum.clone(), SAMPLE_RATE, 4, 7);
        let mut processor = PropagationProcessor::new(&spectrum, &propagated, SAMPLE_RATE, 4);
        let mut frame = HydrophoneFrame::new(1);
        let sample_count = SAMPLE_RATE as usize * DURATION_S;
        let mut source_samples = Vec::with_capacity(sample_count);
        let mut received_samples = Vec::with_capacity(sample_count);
        for index in 0..sample_count {
            let time_s = index as f64 / SAMPLE_RATE as f64;
            let source_sample = voice.sample_at(time_s, 0.0);
            source_samples.push(source_sample.tonal_pressure_1m_upa.iter().sum());
            frame.clear();
            processor.render_into(&voice, time_s, SAMPLE_RATE, &mut frame);
            received_samples.push(frame.pressure_upa[0]);
            voice.advance();
            processor.advance_broadband(&voice);
        }
        let source_fft = power_spectrum(&source_samples, SAMPLE_RATE as f64);
        let received_fft = power_spectrum(&received_samples, SAMPLE_RATE as f64);

        for (index, fields) in golden.iter().enumerate() {
            let frequency_hz = fields[0];
            let source_level_db = fields[1];
            let transmission_loss_db_golden = fields[2];
            let received_level_db = fields[3];
            let frequency_tolerance_hz = fields[4];
            let level_tolerance_db = fields[5];
            let source_line = spectrum.tonal_lines[index];
            let received_line = propagated.tonal_lines[index];

            assert!((source_line.frequency_hz as f64 - frequency_hz).abs() < 0.001);
            assert!((source_line.level_db_re_1upa_at_1m as f64 - source_level_db).abs() < 0.001);
            assert!(
                (transmission_loss_db(RANGE_M, frequency_hz as f32 / 1000.0) as f64
                    - transmission_loss_db_golden)
                    .abs()
                    < 0.0001
            );
            assert!((received_line.level_db_re_1upa as f64 - received_level_db).abs() < 0.001);

            for (label, samples, fft, expected_level) in [
                (
                    "source",
                    source_samples.as_slice(),
                    &source_fft,
                    source_level_db,
                ),
                (
                    "received",
                    received_samples.as_slice(),
                    &received_fft,
                    received_level_db,
                ),
            ] {
                let peak = strongest_peak(
                    fft,
                    frequency_hz - PEAK_SEARCH_HALF_WIDTH_HZ,
                    frequency_hz + PEAK_SEARCH_HALF_WIDTH_HZ,
                )
                .expect("골든 톤 피크");
                let measured_level =
                    tone_rms_level_db(samples, SAMPLE_RATE as f64, frequency_hz).unwrap();
                assert!(
                    (peak.frequency_hz - frequency_hz).abs() <= frequency_tolerance_hz,
                    "{label} {frequency_hz:.3} Hz: peak={:.3}",
                    peak.frequency_hz
                );
                assert!(
                    (measured_level - expected_level).abs() <= level_tolerance_db,
                    "{label} {frequency_hz:.3} Hz: measured={measured_level:.3} expected={expected_level:.3}"
                );
            }
        }
    }

    #[test]
    fn synthesized_merchant_psd_tracks_propagated_band_targets() {
        let sample_rate = 44_100.0;
        let source_spectrum = merchant::source_spectrum(MerchantProfile::Bulker, 13.5, 211.0);
        let propagated = propagate(
            &source_spectrum,
            PropagationGeometry {
                bearing_deg: 0.0,
                range_m: 1000.0,
                source_depth_m: 6.0,
                receiver_depth_m: 6.0,
                relative_velocity_ms: 0.0,
            },
            &[[0.0, 0.0, 0.0]],
            1500.0,
        );
        let golden = include_str!("../../data/acoustics/golden/jomopans_bulker_1km.csv")
            .lines()
            .filter(|line| {
                !line.is_empty() && !line.starts_with('#') && !line.starts_with("frequency_hz")
            })
            .map(|line| {
                let values = line
                    .split(',')
                    .map(|value| value.parse::<f32>().expect("상선 전파 골든 숫자"))
                    .collect::<Vec<_>>();
                assert_eq!(values.len(), 4);
                let source_band = source_spectrum
                    .broadband_bands
                    .iter()
                    .find(|band| (band.center_hz - values[0]).abs() < 0.01)
                    .expect("Source 검증 중심 주파수");
                let received_band = propagated
                    .broadband_bands
                    .iter()
                    .find(|band| (band.center_hz - values[0]).abs() < 0.01)
                    .expect("Propagation 검증 중심 주파수");
                assert!(
                    (source_band.spectrum_level_db_re_1upa2_per_hz_at_1m - values[1]).abs() < 0.002
                );
                assert!(
                    (transmission_loss_db(1000.0, values[0] / 1000.0) - values[2]).abs() < 0.002
                );
                assert!(
                    (received_band.spectrum_level_db_re_1upa2_per_hz - values[3]).abs() < 0.002
                );
                (values[0], values[3])
            })
            .collect::<Vec<_>>();
        let mut source = SourceVoice::new(source_spectrum, sample_rate, 4, 7);
        let mut processor =
            PropagationProcessor::new(source.spectrum(), &propagated, sample_rate, 4);
        let mut frame = HydrophoneFrame::new(1);
        let mut next_sample = || {
            source.advance();
            processor.advance_broadband(&source);
            frame.clear();
            processor.render_into(&source, 0.0, sample_rate, &mut frame);
            frame.pressure_upa[0]
        };
        for _ in 0..32_768 {
            next_sample();
        }
        let samples = (0..262_144).map(|_| next_sample()).collect::<Vec<_>>();
        for (frequency, expected) in golden {
            let measured = measured_psd_db(&samples, sample_rate, frequency);
            assert!(
                (measured - expected).abs() <= 2.5,
                "{frequency} Hz: measured={measured:.2} expected={expected:.2} error={:.2} dB",
                measured - expected
            );
        }
    }

    #[test]
    fn processor_writes_reusable_physical_pressure_frame() {
        let spectrum = source_spectrum(120.0, 5, 120.0, 0.0);
        let propagated = propagate(
            &spectrum,
            PropagationGeometry {
                bearing_deg: 0.0,
                range_m: 1.0,
                source_depth_m: 0.0,
                receiver_depth_m: 0.0,
                relative_velocity_ms: 0.0,
            },
            &[[0.0, 0.0, 0.0]],
            1500.0,
        );
        let processor = PropagationProcessor::new(&spectrum, &propagated, 44_100.0, 4);
        let source = SourceVoice::new(spectrum, 44_100.0, 4, 7);
        let mut frame = HydrophoneFrame::new(1);
        processor.render_into(&source, 0.0, 44100.0, &mut frame);
        assert!(frame.pressure_upa[0] > 1_000_000.0);
        frame.clear();
        assert_eq!(frame.pressure_upa, [0.0]);
    }
}
