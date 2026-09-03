//! 네이티브 오프라인 렌더 결과의 직렬화와 수치 요약.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use crate::DspTraceSample;

#[derive(Debug, Clone, PartialEq)]
pub struct PowerSpectrum {
    pub bin_width_hz: f64,
    pub psd: Vec<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpectralPeak {
    pub frequency_hz: f64,
    pub psd_db: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpectralMetrics {
    pub source_tonal_peak: SpectralPeak,
    pub demon_peak: SpectralPeak,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockMetrics {
    pub sample_count: usize,
    pub rms_fs: f32,
    pub peak_abs_fs: f32,
    pub mean_fs: f32,
    pub zero_crossings: usize,
    pub fnv1a64: u64,
}

pub fn measure_block(samples: &[f32]) -> BlockMetrics {
    let count = samples.len().max(1) as f64;
    let sum = samples.iter().map(|sample| *sample as f64).sum::<f64>();
    let power = samples
        .iter()
        .map(|sample| (*sample as f64).powi(2))
        .sum::<f64>();
    let peak_abs_fs = samples
        .iter()
        .map(|sample| sample.abs())
        .fold(0.0f32, f32::max);
    let zero_crossings = samples
        .windows(2)
        .filter(|pair| (pair[0] < 0.0) != (pair[1] < 0.0))
        .count();

    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in samples.iter().flat_map(|sample| sample.to_le_bytes()) {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }

    BlockMetrics {
        sample_count: samples.len(),
        rms_fs: (power / count).sqrt() as f32,
        peak_abs_fs,
        mean_fs: (sum / count) as f32,
        zero_crossings,
        fnv1a64: hash,
    }
}

#[derive(Clone, Copy, Default)]
struct Complex {
    re: f64,
    im: f64,
}

fn fft(values: &mut [Complex]) {
    let count = values.len();
    debug_assert!(count.is_power_of_two());
    let mut reversed = 0usize;
    for index in 1..count {
        let mut bit = count >> 1;
        while reversed & bit != 0 {
            reversed ^= bit;
            bit >>= 1;
        }
        reversed ^= bit;
        if index < reversed {
            values.swap(index, reversed);
        }
    }

    let mut width = 2;
    while width <= count {
        let angle = -2.0 * std::f64::consts::PI / width as f64;
        let step = Complex {
            re: angle.cos(),
            im: angle.sin(),
        };
        for start in (0..count).step_by(width) {
            let mut twiddle = Complex { re: 1.0, im: 0.0 };
            for offset in 0..width / 2 {
                let even = values[start + offset];
                let odd = values[start + offset + width / 2];
                let rotated = Complex {
                    re: odd.re * twiddle.re - odd.im * twiddle.im,
                    im: odd.re * twiddle.im + odd.im * twiddle.re,
                };
                values[start + offset] = Complex {
                    re: even.re + rotated.re,
                    im: even.im + rotated.im,
                };
                values[start + offset + width / 2] = Complex {
                    re: even.re - rotated.re,
                    im: even.im - rotated.im,
                };
                twiddle = Complex {
                    re: twiddle.re * step.re - twiddle.im * step.im,
                    im: twiddle.re * step.im + twiddle.im * step.re,
                };
            }
        }
        width *= 2;
    }
}

/// Hann 윈도우를 적용한 단측 전력 스펙트럼 밀도(입력 단위²/Hz)를 계산한다.
///
/// FFT 길이는 다음 2의 거듭제곱으로 영 패딩하지만 정규화에는 실제 Hann 윈도우
/// 전력을 사용하므로 적분 전력은 패딩 길이에 좌우되지 않는다.
pub fn power_spectrum(samples: &[f32], sample_rate_hz: f64) -> PowerSpectrum {
    if samples.len() < 2 || !sample_rate_hz.is_finite() || sample_rate_hz <= 0.0 {
        return PowerSpectrum {
            bin_width_hz: 0.0,
            psd: Vec::new(),
        };
    }
    let fft_len = samples.len().next_power_of_two();
    let mut values = vec![Complex::default(); fft_len];
    let denominator = (samples.len() - 1) as f64;
    let mut window_power = 0.0;
    for (index, sample) in samples.iter().enumerate() {
        let window = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * index as f64 / denominator).cos();
        values[index].re = *sample as f64 * window;
        window_power += window * window;
    }
    fft(&mut values);

    let last_bin = fft_len / 2;
    let mut psd = Vec::with_capacity(last_bin + 1);
    for (bin, value) in values[..=last_bin].iter().enumerate() {
        let mut density =
            (value.re * value.re + value.im * value.im) / (sample_rate_hz * window_power);
        if bin != 0 && bin != last_bin {
            density *= 2.0;
        }
        psd.push(density);
    }
    PowerSpectrum {
        bin_width_hz: sample_rate_hz / fft_len as f64,
        psd,
    }
}

/// 지정 대역에서 가장 강한 PSD 빈을 찾고 로그 PSD의 포물선 보간으로 주파수를 다듬는다.
pub fn strongest_peak(
    spectrum: &PowerSpectrum,
    minimum_hz: f64,
    maximum_hz: f64,
) -> Option<SpectralPeak> {
    if spectrum.psd.is_empty() || spectrum.bin_width_hz <= 0.0 || maximum_hz < minimum_hz {
        return None;
    }
    let first = (minimum_hz / spectrum.bin_width_hz).ceil().max(0.0) as usize;
    let last = ((maximum_hz / spectrum.bin_width_hz).floor() as usize).min(spectrum.psd.len() - 1);
    if first > last {
        return None;
    }
    let bin = (first..=last)
        .max_by(|left, right| spectrum.psd[*left].total_cmp(&spectrum.psd[*right]))?;
    let mut offset = 0.0;
    if bin > 0 && bin + 1 < spectrum.psd.len() {
        let left = spectrum.psd[bin - 1].max(f64::MIN_POSITIVE).log10();
        let center = spectrum.psd[bin].max(f64::MIN_POSITIVE).log10();
        let right = spectrum.psd[bin + 1].max(f64::MIN_POSITIVE).log10();
        let curvature = left - 2.0 * center + right;
        if curvature.abs() > f64::EPSILON {
            offset = (0.5 * (left - right) / curvature).clamp(-0.5, 0.5);
        }
    }
    Some(SpectralPeak {
        frequency_hz: (bin as f64 + offset) * spectrum.bin_width_hz,
        psd_db: 10.0 * spectrum.psd[bin].max(f64::MIN_POSITIVE).log10(),
    })
}

/// 광대역 압력의 제곱 포락선에서 DC를 제거한 DEMON PSD를 계산한다.
pub fn demon_spectrum(samples: &[f32], sample_rate_hz: f64) -> PowerSpectrum {
    if samples.is_empty() {
        return power_spectrum(samples, sample_rate_hz);
    }
    let mean_square = samples
        .iter()
        .map(|sample| (*sample as f64).powi(2))
        .sum::<f64>()
        / samples.len() as f64;
    let envelope = samples
        .iter()
        .map(|sample| ((*sample as f64).powi(2) - mean_square) as f32)
        .collect::<Vec<_>>();
    power_spectrum(&envelope, sample_rate_hz)
}

/// IEEE float32 mono WAV를 만들어 DSP 출력을 양자화 없이 보존한다.
pub fn encode_float_wav(sample_rate: u32, samples: &[f32]) -> Vec<u8> {
    let data_len = u32::try_from(samples.len().saturating_mul(4)).expect("WAV data too large");
    let sample_count = u32::try_from(samples.len()).expect("WAV sample count too large");
    let mut wav = Vec::with_capacity(56 + data_len as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(48 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&3u16.to_le_bytes()); // WAVE_FORMAT_IEEE_FLOAT
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 4).to_le_bytes());
    wav.extend_from_slice(&4u16.to_le_bytes());
    wav.extend_from_slice(&32u16.to_le_bytes());
    wav.extend_from_slice(b"fact");
    wav.extend_from_slice(&4u32.to_le_bytes());
    wav.extend_from_slice(&sample_count.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    wav
}

pub fn write_float_wav(path: &Path, sample_rate: u32, samples: &[f32]) -> io::Result<()> {
    std::fs::write(path, encode_float_wav(sample_rate, samples))
}

pub fn write_trace_csv(path: &Path, trace: &[DspTraceSample]) -> io::Result<()> {
    let mut writer = BufWriter::new(File::create(path)?);
    writeln!(
        writer,
        "sample,source_1m_upa,source_tonal_1m_upa,source_broadband_1m_upa,hydrophone_0_upa,receiver_fs,output_fs"
    )?;
    for (index, sample) in trace.iter().enumerate() {
        writeln!(
            writer,
            "{index},{:.6},{:.6},{:.6},{:.6},{:.9},{:.9}",
            sample.source_1m_upa,
            sample.source_tonal_1m_upa,
            sample.source_broadband_1m_upa,
            sample.hydrophone_0_upa,
            sample.receiver_fs,
            sample.output_fs
        )?;
    }
    writer.flush()
}

pub fn write_summary_json(
    path: &Path,
    sample_rate: u32,
    metrics: BlockMetrics,
    spectral: SpectralMetrics,
) -> io::Result<()> {
    let mut writer = BufWriter::new(File::create(path)?);
    writeln!(writer, "{{")?;
    writeln!(writer, "  \"format\": \"f32le-mono\",")?;
    writeln!(writer, "  \"sample_rate_hz\": {sample_rate},")?;
    writeln!(writer, "  \"sample_count\": {},", metrics.sample_count)?;
    writeln!(writer, "  \"rms_fs\": {:.9},", metrics.rms_fs)?;
    writeln!(writer, "  \"peak_abs_fs\": {:.9},", metrics.peak_abs_fs)?;
    writeln!(writer, "  \"mean_fs\": {:.9},", metrics.mean_fs)?;
    writeln!(writer, "  \"zero_crossings\": {},", metrics.zero_crossings)?;
    writeln!(writer, "  \"fnv1a64\": \"{:016x}\",", metrics.fnv1a64)?;
    writeln!(
        writer,
        "  \"source_tonal_peak_hz\": {:.6},",
        spectral.source_tonal_peak.frequency_hz
    )?;
    writeln!(
        writer,
        "  \"source_tonal_peak_psd_db\": {:.6},",
        spectral.source_tonal_peak.psd_db
    )?;
    writeln!(
        writer,
        "  \"demon_peak_hz\": {:.6},",
        spectral.demon_peak.frequency_hz
    )?;
    writeln!(
        writer,
        "  \"demon_peak_psd_db\": {:.6}",
        spectral.demon_peak.psd_db
    )?;
    writeln!(writer, "}}")?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DspEngine;

    #[test]
    fn metrics_and_hash_are_deterministic() {
        let samples = [-0.5, 0.25, -0.125, 0.0, 1.0];
        let first = measure_block(&samples);
        let second = measure_block(&samples);
        assert_eq!(first, second);
        assert_eq!(first.sample_count, 5);
        assert_eq!(first.peak_abs_fs, 1.0);
        assert_eq!(first.zero_crossings, 3);
        assert_ne!(first.fnv1a64, 0);
    }

    #[test]
    fn wav_header_describes_float32_mono_samples() {
        let wav = encode_float_wav(44_100, &[0.25, -0.5]);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(u16::from_le_bytes([wav[20], wav[21]]), 3);
        assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 1);
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 44_100);
        assert_eq!(&wav[36..40], b"fact");
        assert_eq!(u32::from_le_bytes(wav[44..48].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(wav[52..56].try_into().unwrap()), 8);
        assert_eq!(&wav[56..60], &0.25f32.to_le_bytes());
    }

    #[test]
    fn hann_psd_finds_sine_frequency_and_preserves_power() {
        let sample_rate = 1024.0;
        let frequency = 73.25;
        let samples = (0..2048)
            .map(|index| {
                (2.0 * std::f64::consts::PI * frequency * index as f64 / sample_rate).sin() as f32
            })
            .collect::<Vec<_>>();
        let spectrum = power_spectrum(&samples, sample_rate);
        let peak = strongest_peak(&spectrum, 60.0, 90.0).unwrap();
        let integrated_power = spectrum.psd.iter().sum::<f64>() * spectrum.bin_width_hz;

        assert!((peak.frequency_hz - frequency).abs() < 0.05);
        assert!((integrated_power - 0.5).abs() < 0.001);
    }

    #[test]
    fn demon_psd_recovers_amplitude_modulation_rate() {
        let sample_rate = 4096.0;
        let modulation_hz = 13.0;
        let samples = (0..8192)
            .map(|index| {
                let time = index as f64 / sample_rate;
                let envelope =
                    0.5 + 0.5 * (2.0 * std::f64::consts::PI * modulation_hz * time).cos();
                (envelope * (2.0 * std::f64::consts::PI * 701.0 * time).sin()) as f32
            })
            .collect::<Vec<_>>();
        let peak = strongest_peak(&demon_spectrum(&samples, sample_rate), 5.0, 30.0).unwrap();

        assert!((peak.frequency_hz - modulation_hz).abs() < 0.1);
    }

    #[test]
    fn reference_scene_matches_golden_block_metrics() {
        let row = include_str!("../../data/acoustics/golden/dsp_reference_scene.csv")
            .lines()
            .nth(1)
            .unwrap();
        let fields = row.split(',').collect::<Vec<_>>();
        let sample_rate = fields[0].parse::<u32>().unwrap();
        let warmup_seconds = fields[1].parse::<f32>().unwrap();
        let duration_seconds = fields[2].parse::<f32>().unwrap();
        let expected_rms = fields[3].parse::<f32>().unwrap();
        let rms_tolerance = fields[4].parse::<f32>().unwrap();
        let expected_peak = fields[5].parse::<f32>().unwrap();
        let peak_tolerance = fields[6].parse::<f32>().unwrap();
        let expected_crossings = fields[7].parse::<usize>().unwrap();
        let crossing_tolerance = fields[8].parse::<usize>().unwrap();
        let expected_tonal_hz = fields[9].parse::<f64>().unwrap();
        let tonal_tolerance_hz = fields[10].parse::<f64>().unwrap();
        let expected_demon_hz = fields[11].parse::<f64>().unwrap();
        let demon_tolerance_hz = fields[12].parse::<f64>().unwrap();

        let mut engine = DspEngine::new(sample_rate as f32);
        engine.set_targets(&[0.0, 1000.0, 30.0, 120.0, 5.0, 155.0, 0.4, 5.0]);
        engine.set_beam(0.0, 0.0);
        let mut warmup = vec![0.0; (warmup_seconds * sample_rate as f32) as usize];
        engine.process(&mut warmup);
        let mut output = vec![0.0; (duration_seconds * sample_rate as f32) as usize];
        let mut trace = vec![DspTraceSample::default(); output.len()];
        engine.process_traced(&mut output, &mut trace);
        let actual = measure_block(&output);
        let tonal = trace
            .iter()
            .map(|sample| sample.source_tonal_1m_upa)
            .collect::<Vec<_>>();
        let broadband = trace
            .iter()
            .map(|sample| sample.source_broadband_1m_upa)
            .collect::<Vec<_>>();
        let tonal_peak =
            strongest_peak(&power_spectrum(&tonal, sample_rate as f64), 5.0, 20.0).unwrap();
        let demon_peak =
            strongest_peak(&demon_spectrum(&broadband, sample_rate as f64), 5.0, 20.0).unwrap();

        assert!((actual.rms_fs - expected_rms).abs() <= rms_tolerance);
        assert!((actual.peak_abs_fs - expected_peak).abs() <= peak_tolerance);
        assert!(actual.zero_crossings.abs_diff(expected_crossings) <= crossing_tolerance);
        assert!((tonal_peak.frequency_hz - expected_tonal_hz).abs() <= tonal_tolerance_hz);
        assert!((demon_peak.frequency_hz - expected_demon_hz).abs() <= demon_tolerance_hz);
    }
}
