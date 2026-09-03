//! 전파 결과를 DSP full-scale 진폭으로 변환하는 수신기 계층.
//!
//! 입력의 TL·도플러·공간 지연을 변경하지 않으며, 센서 감도/동역학 기준만 적용한다.

use crate::beamform::DelayAndSum;
use crate::noise::OceanNoise;
use crate::propagation::{HydrophoneFrame, PropagatedSpectrum};
use crate::source::TONAL_HARMONICS;

/// 기본 수신기 full-scale: 1 Pa = 1,000,000 µPa = 120 dB re 1 µPa.
pub const DEFAULT_FULL_SCALE_DB_RE_1UPA: f32 = 120.0;

#[derive(Debug, Clone, PartialEq)]
pub struct ReceiverVoiceParameters {
    pub tonal_frequency_hz: [f32; TONAL_HARMONICS as usize],
    pub tonal_amplitude_fs: [f32; TONAL_HARMONICS as usize],
    pub broadband_amplitude_fs: f32,
    pub modulation_rate_hz: f32,
    pub hydrophone_delays_s: Vec<f32>,
    pub arrival_direction: [f32; 3],
}

/// 하이드로폰 물리 음압을 full-scale로 변환하고 배열·주변 소음을 처리하는 수신기.
#[derive(Debug, Clone)]
pub struct ReceiverArray {
    array: DelayAndSum,
    ambient: OceanNoise,
    pressure_to_full_scale: f32,
    full_scale_frame: Vec<f32>,
}

impl ReceiverArray {
    pub fn new(
        hydrophones: Vec<[f32; 3]>,
        sound_speed_ms: f32,
        sample_rate: f32,
        full_scale_db_re_1upa: f32,
    ) -> Self {
        let hydrophone_count = hydrophones.len();
        Self {
            array: DelayAndSum::new(hydrophones, sound_speed_ms, sample_rate),
            ambient: OceanNoise::new(sample_rate, 5.0, 0.0),
            pressure_to_full_scale: 10f32.powf(-full_scale_db_re_1upa / 20.0),
            full_scale_frame: vec![0.0; hydrophone_count],
        }
    }

    pub fn set_ocean(&mut self, sample_rate: f32, wind_speed_ms: f32, rain_mm_hr: f32) {
        self.ambient = OceanNoise::new(sample_rate, wind_speed_ms, rain_mm_hr);
    }

    /// 배열 신호만 처리한다. 주변 소음과 출력 제한은 포함하지 않는다.
    #[inline]
    pub fn process_signal_frame(&mut self, frame: &HydrophoneFrame, steer: [f32; 3]) -> f32 {
        debug_assert_eq!(frame.pressure_upa.len(), self.full_scale_frame.len());
        for (output, pressure_upa) in self.full_scale_frame.iter_mut().zip(&frame.pressure_upa) {
            *output = pressure_upa * self.pressure_to_full_scale;
        }
        self.array.process_sample(&self.full_scale_frame, steer)
    }

    /// 배열 출력에 수신점 주변 소음을 합성한다.
    #[inline]
    pub fn process_frame(&mut self, frame: &HydrophoneFrame, steer: [f32; 3]) -> f32 {
        self.process_signal_frame(frame, steer) + self.ambient.next_sample()
    }

    pub fn beam_sample(&self, steer: [f32; 3]) -> f32 {
        self.array.beam_sample(steer)
    }
}

#[inline]
pub fn pressure_level_to_full_scale(level_db_re_1upa: f32, full_scale_db_re_1upa: f32) -> f32 {
    10f32.powf((level_db_re_1upa - full_scale_db_re_1upa) / 20.0)
}

pub fn receive(
    propagated: &PropagatedSpectrum,
    full_scale_db_re_1upa: f32,
) -> ReceiverVoiceParameters {
    ReceiverVoiceParameters {
        tonal_frequency_hz: std::array::from_fn(|index| propagated.tonal_lines[index].frequency_hz),
        tonal_amplitude_fs: std::array::from_fn(|index| {
            pressure_level_to_full_scale(
                propagated.tonal_lines[index].level_db_re_1upa,
                full_scale_db_re_1upa,
            )
        }),
        broadband_amplitude_fs: pressure_level_to_full_scale(
            propagated.broadband_level_db_re_1upa,
            full_scale_db_re_1upa,
        ),
        modulation_rate_hz: propagated.modulation_rate_hz,
        hydrophone_delays_s: propagated.hydrophone_delays_s.clone(),
        arrival_direction: propagated.arrival_direction,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::propagation::{PropagatedSpectrum, ReceivedLine};

    fn propagated_fixture() -> PropagatedSpectrum {
        PropagatedSpectrum {
            tonal_lines: std::array::from_fn(|index| ReceivedLine {
                frequency_hz: 10.0 * (index + 1) as f32,
                level_db_re_1upa: 100.0 - index as f32 * 6.0,
            }),
            broadband_bands: Vec::new(),
            broadband_level_db_re_1upa: 90.0,
            modulation_rate_hz: 10.0,
            hydrophone_delays_s: vec![0.0, 0.001],
            arrival_direction: [1.0, 0.0, 0.0],
        }
    }

    #[test]
    fn receiver_scale_does_not_mutate_propagation() {
        let propagated = propagated_fixture();
        let original = propagated.clone();
        let normal = receive(&propagated, 120.0);
        let sensitive = receive(&propagated, 100.0);
        assert_eq!(propagated, original);
        assert_eq!(normal.tonal_frequency_hz, sensitive.tonal_frequency_hz);
        assert!(
            (sensitive.tonal_amplitude_fs[0] / normal.tonal_amplitude_fs[0] - 10.0).abs() < 1e-5
        );
    }

    #[test]
    fn receiver_preserves_spatial_contract() {
        let propagated = propagated_fixture();
        let received = receive(&propagated, DEFAULT_FULL_SCALE_DB_RE_1UPA);
        assert_eq!(received.hydrophone_delays_s, propagated.hydrophone_delays_s);
        assert_eq!(received.arrival_direction, propagated.arrival_direction);
    }

    #[test]
    fn receiver_converts_upa_frame_to_full_scale() {
        let mut receiver = ReceiverArray::new(
            vec![[0.0, 0.0, 0.0]],
            1500.0,
            44100.0,
            DEFAULT_FULL_SCALE_DB_RE_1UPA,
        );
        let frame = HydrophoneFrame {
            pressure_upa: vec![1_000_000.0],
        };
        receiver.process_signal_frame(&frame, [1.0, 0.0, 0.0]);
        let output = receiver.process_signal_frame(&frame, [1.0, 0.0, 0.0]);
        assert!((output - 1.0).abs() < 1e-6);
    }
}
