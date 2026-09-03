//! Source 스펙트럼에 거리·도플러·배열 도달 지연을 적용하는 전파 계층.
//!
//! 입력은 `dB re 1 µPa @ 1 m`, 출력은 수신점의 `dB re 1 µPa`다.
//! full-scale 변환과 빔포밍은 Receiver 계층의 책임이다.

use crate::beamform::beam_delay;
use crate::physics::transmission_loss_db;
use crate::source::{SourceLine, SourceSpectrum, SourceVoice, TONAL_HARMONICS};

/// 캐비테이션 광대역 TL을 평가하는 기준 주파수 (kHz).
pub const BROADBAND_REFERENCE_KHZ: f32 = 1.0;

/// 상대 속도에 따른 도플러 주파수 계수. 수신기 방향 접근이 양수다.
#[inline]
pub fn doppler_factor(relative_velocity_ms: f32, sound_speed_ms: f32) -> f32 {
    1.0 + relative_velocity_ms / sound_speed_ms
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

/// 단일 표적이 수신 배열 위치에 만든 스펙트럼과 공간 지연.
#[derive(Debug, Clone, PartialEq)]
pub struct PropagatedSpectrum {
    pub tonal_lines: [ReceivedLine; TONAL_HARMONICS as usize],
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
    doppler_factor: f32,
    hydrophone_delays_s: Vec<f32>,
}

impl PropagationProcessor {
    pub fn new(source: &SourceSpectrum, propagated: &PropagatedSpectrum) -> Self {
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
        let doppler_factor = propagated.tonal_lines[0].frequency_hz
            / source.tonal_lines[0].frequency_hz.max(f32::MIN_POSITIVE);
        Self {
            tonal_linear_loss,
            broadband_linear_loss,
            doppler_factor,
            hydrophone_delays_s: propagated.hydrophone_delays_s.clone(),
        }
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
            frame.pressure_upa[hydrophone] +=
                tonal + sample.broadband_pressure_1m_upa * self.broadband_linear_loss;
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
        } = source.tonal_lines[index];
        let shifted_frequency_hz = frequency_hz * doppler;
        ReceivedLine {
            frequency_hz: shifted_frequency_hz,
            level_db_re_1upa: level_db_re_1upa_at_1m
                - transmission_loss_db(range_m, shifted_frequency_hz / 1000.0),
        }
    });
    let broadband_level_db_re_1upa = source.broadband_level_db_re_1upa_at_1m
        - transmission_loss_db(range_m, BROADBAND_REFERENCE_KHZ);

    PropagatedSpectrum {
        tonal_lines,
        broadband_level_db_re_1upa,
        modulation_rate_hz: source.modulation_rate_hz * doppler,
        hydrophone_delays_s,
        arrival_direction,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::source_spectrum;

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
        let processor = PropagationProcessor::new(&spectrum, &propagated);
        let source = SourceVoice::new(spectrum, 44_100.0, 4, 7);
        let mut frame = HydrophoneFrame::new(1);
        processor.render_into(&source, 0.0, 44100.0, &mut frame);
        assert!(frame.pressure_upa[0] > 1_000_000.0);
        frame.clear();
        assert_eq!(frame.pressure_upa, [0.0]);
    }
}
