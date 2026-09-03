# DSP 계층 계약

> 목적: 발생원, 전파 환경, 수신기를 독립적으로 교체·검증하기 위한 M2B 경계 정의.
> 구현: `dsp-core/src/source/mod.rs`, `propagation.rs`, `receiver.rs`.

## 1. 계층별 소유권

| 계층 | 입력 | 출력 | 포함 | 금지 |
|---|---|---|---|---|
| Source | `source_profile_id`, 운항 상태 또는 기존 RPM·준위 제어 | `SourceSpectrum`, `SourceSample` | 1 m 방사 톤선, 대역별 광대역 PSD, 자체 변조율과 결정적 잡음 히스토리 | 거리 TL, 도플러, 하이드로폰 지연, 수신기 감도 |
| Propagation | `SourceVoice`, `PropagationGeometry`, 배열 위치, 음속 | `PropagatedSpectrum`, `HydrophoneFrame` | 주파수별 TL, 도플러, 도달 방향, 인과적 배열 지연과 µPa 프레임 혼합 | full-scale 정규화, 빔 조향, 후단 압축 |
| Receiver | `HydrophoneFrame`, full-scale 기준, 배열/해양 상태 | 조향 빔 샘플 | µPa → 선형 FS 변환, 배열 지연-합, 수신점 주변 소음 | 소스 준위 변경, TL·도플러 재계산 |
| Analysis | 수신기 배열/빔 샘플 | BASS/LOFAR 텔레메트리 | 감산, 전력·스펙트럼 분석, 표시용 집계 | 시간축 전진, 음원 재합성, 전파·수신 효과 변경 |
| Output | 주 조향 빔 샘플 | 오디오 샘플 | 출력 제한과 향후 게인·라우팅 | 분석 결과를 이용한 원 신호 변조 |

## 2. 단위 계약

| 필드 | 단위/규약 |
|---|---|
| `SourceLine::frequency_hz` | Hz, 도플러 적용 전 |
| `SourceLine::level_db_re_1upa_at_1m` | dB re 1 µPa @ 1 m |
| `SourceBand::center_hz` | Hz, 도플러 적용 전 대역 중심 |
| `SourceBand::spectrum_level_db_re_1upa2_per_hz_at_1m` | dB re 1 µPa²/Hz @ 1 m |
| `ReceivedLine::frequency_hz` | Hz, 도플러 적용 후 |
| `ReceivedLine::level_db_re_1upa` | 수신점 dB re 1 µPa |
| `hydrophone_delays_s` | s, 공통 `pmax` 이동 후 0 이상 |
| `HydrophoneFrame::pressure_upa` | 하이드로폰별 수신점 선형 음압, µPa |
| `*_amplitude_fs` | 선형 진폭, 기본 120 dB re 1 µPa = 1.0 FS |
| `arrival_direction` | 수신기→소스 단위 벡터, x=전방·y=하향·z=우현 |

`PropagationGeometry`는 소스와 수신기 수심을 각각 받아 수직 오프셋을 계산한다. 현재
8-float WASM 장면 계약에는 자함 수심이 없으므로 호환 경로에서는 수신기 수심을 0 m로
둔다. 월드 어댑터를 연결할 때 자함 수심을 명시적으로 전달하도록 계약을 확장한다.

근거 기반 상선 계약은 표적당 7 float
`[bearing_deg, range_m, depth_m, profile_code, speed_kn, length_m, rel_vel_ms]`다.
프로파일 코드는 1=벌크선, 2=컨테이너선, 3=차량운반선, 4=탱커이며 TS의 문자열 ID는
`src/dsp/sourceProfiles.ts`에서만 이 코드로 변환한다. 기존 8-float 계약은 회귀·비교용으로
유지하며 새 상선 장면에는 사용하지 않는다.

## 3. 불변 조건과 자동 검증

- 거리만 변경하면 Source 프레임은 동일하고 Propagation 수신 준위만 감소한다.
- 상대 속도만 변경하면 Source 프레임은 동일하고 전파 후 주파수·변조율만 변한다.
- 수신기 full-scale 기준만 변경하면 전파 결과는 동일하고 선형 진폭만 변한다.
- Receiver는 전파 계층의 주파수, 도달 방향, 하이드로폰 지연을 보존한다.
- 기존 엔진 결정성과 거리 감쇠·RPM·도플러·빔포밍 회귀 테스트를 계속 통과해야 한다.

## 4. 상태형 샘플 경로와 다음 분리

스펙트럼 제어 프레임뿐 아니라 상태형 실시간 샘플 경로에도 같은 경계를 적용했다.

1. `SourceVoice`가 1 m 기준 토널과 결정적 31대역 광대역 `SourceSample`을 µPa로 생성한다.
2. `PropagationProcessor`가 각 Source 대역에 중심 주파수별 Thorp 흡수·거리 TL을
   먼저 적용해 전파 지연 히스토리에 합산하고, 토널과 함께 재사용
   `HydrophoneFrame`에 수신점 µPa를 누적한다.
3. `ReceiverArray`가 µPa를 full-scale로 교정하고 주변 소음과 배열 응답을 적용한다.
4. `DspEngine`은 위 객체를 순서대로 호출하며 샘플 루프에서 버퍼를 할당하지 않는다.

`BassAnalyzer`는 배열의 최신 링 버퍼를 시간축 전진 없이 방위별로 읽고, 감산 주기·전력
누산·dBFS 변환·윈도우 초기화를 소유한다. `output::soft_limit`는 주 조향 빔에만 적용되므로
분석 입력을 변조하지 않는다. `DspEngine`은 두 경로의 실행 순서와 WASM 계약만 조율한다.

네이티브 전용 `process_traced`는 같은 내부 샘플 함수를 호출하면서 Source 1 m의 전체·
토널·광대역 음압, 첫 하이드로폰 음압, 제한 전 수신기 FS, 최종 출력 FS를 기록한다.
`dsp:render`는 이를 float32 WAV·CSV·요약 JSON으로 저장하며 기준 장면 블록 통계와
Source 토널·DEMON 피크 주파수를 골든 테스트로 비교한다.
세부 규약은 `docs/오프라인 DSP 검증.md`를 따른다.

상선 합성 PSD는 1 km 골든에서 `±2.5 dB`로 검사하고 50 km에서는 고주파 TL이 저주파보다
추가로 감소하는 수식을 검증한다. 매우 먼 거리의 고주파 성분은 f32 물리 압력 합산의 수치
바닥 아래로 내려가므로 고정밀 오프라인 분석과 실시간 청취 경로의 요구를 구분한다.
다음 단계에서는 톤 레벨과 채널 응답을 문헌 기반 허용 오차와 비교한다. AudioWorklet의
`DspEngine::process()` 블록당 1회 호출 규약은 계속 유지한다.
