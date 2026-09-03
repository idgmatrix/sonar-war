use std::error::Error;
use std::path::{Path, PathBuf};

use dsp_core::offline::{
    demon_spectrum, measure_block, power_spectrum, strongest_peak, write_float_wav,
    write_summary_json, write_trace_csv, SpectralMetrics,
};
use dsp_core::{DspEngine, DspTraceSample};

const DEFAULT_SAMPLE_RATE: u32 = 44_100;
const DEFAULT_SECONDS: f32 = 1.0;
const DEFAULT_WARMUP_SECONDS: f32 = 0.25;

#[derive(Debug)]
struct Options {
    output_prefix: PathBuf,
    sample_rate: u32,
    seconds: f32,
    warmup_seconds: f32,
}

fn parse_options() -> Result<Option<Options>, String> {
    let mut options = Options {
        output_prefix: PathBuf::from("artifacts/offline/reference_scene"),
        sample_rate: DEFAULT_SAMPLE_RATE,
        seconds: DEFAULT_SECONDS,
        warmup_seconds: DEFAULT_WARMUP_SECONDS,
    };
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--out" => {
                options.output_prefix =
                    PathBuf::from(args.next().ok_or("--out 뒤에 파일 접두사가 필요합니다")?);
            }
            "--sample-rate" => {
                options.sample_rate = args
                    .next()
                    .ok_or("--sample-rate 뒤에 값이 필요합니다")?
                    .parse()
                    .map_err(|_| "sample rate는 양의 정수여야 합니다")?;
            }
            "--seconds" => {
                options.seconds = args
                    .next()
                    .ok_or("--seconds 뒤에 값이 필요합니다")?
                    .parse()
                    .map_err(|_| "seconds는 양수여야 합니다")?;
            }
            "--warmup-seconds" => {
                options.warmup_seconds = args
                    .next()
                    .ok_or("--warmup-seconds 뒤에 값이 필요합니다")?
                    .parse()
                    .map_err(|_| "warmup seconds는 0 이상이어야 합니다")?;
            }
            "--help" | "-h" => return Ok(None),
            _ => return Err(format!("알 수 없는 인자: {argument}")),
        }
    }
    if options.sample_rate == 0 || options.seconds <= 0.0 || options.warmup_seconds < 0.0 {
        return Err("sample rate와 seconds 범위를 확인하세요".into());
    }
    Ok(Some(options))
}

fn with_extension(prefix: &Path, extension: &str) -> PathBuf {
    let mut path = prefix.to_path_buf();
    path.set_extension(extension);
    path
}

fn print_usage() {
    println!(
        "사용법: cargo run --release --example render_scene -- \\
         [--out PREFIX] [--seconds N] [--warmup-seconds N] [--sample-rate HZ]"
    );
}

fn main() -> Result<(), Box<dyn Error>> {
    let Some(options) = parse_options().inspect_err(|_| {
        print_usage();
    })?
    else {
        print_usage();
        return Ok(());
    };

    if let Some(parent) = options.output_prefix.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut engine = DspEngine::new(options.sample_rate as f32);
    // 공개 파라미터 계약의 단일 기준 장면: 정면 1 km, 120 RPM, 5엽, 접근 5 m/s.
    engine.set_targets(&[0.0, 1000.0, 30.0, 120.0, 5.0, 155.0, 0.4, 5.0]);
    engine.set_beam(0.0, 0.0);

    let warmup_samples = (options.warmup_seconds * options.sample_rate as f32) as usize;
    engine.process(&mut vec![0.0; warmup_samples]);

    let sample_count = (options.seconds * options.sample_rate as f32) as usize;
    let mut output = vec![0.0; sample_count];
    let mut trace = vec![DspTraceSample::default(); sample_count];
    engine.process_traced(&mut output, &mut trace);

    let wav_path = with_extension(&options.output_prefix, "wav");
    let trace_path = with_extension(&options.output_prefix, "trace.csv");
    let summary_path = with_extension(&options.output_prefix, "summary.json");
    let metrics = measure_block(&output);
    let tonal = trace
        .iter()
        .map(|sample| sample.source_tonal_1m_upa)
        .collect::<Vec<_>>();
    let broadband = trace
        .iter()
        .map(|sample| sample.source_broadband_1m_upa)
        .collect::<Vec<_>>();
    let spectral = SpectralMetrics {
        source_tonal_peak: strongest_peak(
            &power_spectrum(&tonal, options.sample_rate as f64),
            5.0,
            20.0,
        )
        .ok_or("토널 검색 대역에 FFT 빈이 없습니다")?,
        demon_peak: strongest_peak(
            &demon_spectrum(&broadband, options.sample_rate as f64),
            5.0,
            20.0,
        )
        .ok_or("DEMON 검색 대역에 FFT 빈이 없습니다")?,
    };
    write_float_wav(&wav_path, options.sample_rate, &output)?;
    write_trace_csv(&trace_path, &trace)?;
    write_summary_json(&summary_path, options.sample_rate, metrics, spectral)?;

    println!("WAV: {}", wav_path.display());
    println!("trace: {}", trace_path.display());
    println!("summary: {}", summary_path.display());
    println!(
        "samples={} rms={:.6} peak={:.6} zero_crossings={} fnv1a64={:016x}",
        metrics.sample_count,
        metrics.rms_fs,
        metrics.peak_abs_fs,
        metrics.zero_crossings,
        metrics.fnv1a64
    );
    println!(
        "source_tonal_peak={:.3}Hz demon_peak={:.3}Hz",
        spectral.source_tonal_peak.frequency_hz, spectral.demon_peak.frequency_hz
    );
    Ok(())
}
