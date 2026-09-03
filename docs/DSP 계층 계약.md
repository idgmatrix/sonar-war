# DSP 계층 계약

> 목적: 발생원, 전파 환경, 수신기를 독립적으로 교체·검증하기 위한 M2B 경계 정의.
> 구현: `dsp-core/src/source/mod.rs`, `propagation.rs`, `receiver.rs`.

## 1. 계층별 소유권

| 계층 | 입력 | 출력 | 포함 | 금지 |
|---|---|---|---|---|
| Source | RPM, 블레이드 수, 1 m 토널 준위, 캐비테이션 상태 | `SourceSpectrum`, `SourceSample` | 1 m 방사 톤선, 광대역 준위, 자체 변조율과 결정적 잡음 히스토리 | 거리 TL, 도플러, 하이드로폰 지연, 수신기 감도 |
| Propagation | `SourceVoice`, `PropagationGeometry`, 배열 위치, 음속 | `PropagatedSpectrum`, `HydrophoneFrame` | 주파수별 TL, 도플러, 도달 방향, 인과적 배열 지연과 µPa 프레임 혼합 | full-scale 정규화, 빔 조향, 후단 압축 |
| Receiver | `HydrophoneFrame`, full-scale 기준, 배열/해양 상태 | 조향 빔 샘플 | µPa → 선형 FS 변환, 배열 지연-합, 수신점 주변 소음 | 소스 준위 변경, TL·도플러 재계산 |
| Analysis/Output | 수신기 빔 샘플 | BASS/LOFAR 텔레메트리, 오디오 | 표시 분석, 출력 제한 | 음원 재합성, 전파·수신 효과 변경 |

## 2. 단위 계약

| 필드 | 단위/규약 |
|---|---|
| `SourceLine::frequency_hz` | Hz, 도플러 적용 전 |
| `SourceLine::level_db_re_1upa_at_1m` | dB re 1 µPa @ 1 m |
| `ReceivedLine::frequency_hz` | Hz, 도플러 적용 후 |
| `ReceivedLine::level_db_re_1upa` | 수신점 dB re 1 µPa |
| `hydrophone_delays_s` | s, 공통 `pmax` 이동 후 0 이상 |
| `HydrophoneFrame::pressure_upa` | 하이드로폰별 수신점 선형 음압, µPa |
| `*_amplitude_fs` | 선형 진폭, 기본 120 dB re 1 µPa = 1.0 FS |
| `arrival_direction` | 수신기→소스 단위 벡터, x=전방·y=하향·z=우현 |

`PropagationGeometry`는 소스와 수신기 수심을 각각 받아 수직 오프셋을 계산한다. 현재
8-float WASM 장면 계약에는 자함 수심이 없으므로 호환 경로에서는 수신기 수심을 0 m로
둔다. 월드 어댑터를 연결할 때 자함 수심을 명시적으로 전달하도록 계약을 확장한다.

## 3. 불변 조건과 자동 검증

- 거리만 변경하면 Source 프레임은 동일하고 Propagation 수신 준위만 감소한다.
- 상대 속도만 변경하면 Source 프레임은 동일하고 전파 후 주파수·변조율만 변한다.
- 수신기 full-scale 기준만 변경하면 전파 결과는 동일하고 선형 진폭만 변한다.
- Receiver는 전파 계층의 주파수, 도달 방향, 하이드로폰 지연을 보존한다.
- 기존 엔진 결정성과 거리 감쇠·RPM·도플러·빔포밍 회귀 테스트를 계속 통과해야 한다.

## 4. 상태형 샘플 경로와 다음 분리

스펙트럼 제어 프레임뿐 아니라 상태형 실시간 샘플 경로에도 같은 경계를 적용했다.

1. `SourceVoice`가 1 m 기준 토널·캐비테이션 `SourceSample`을 µPa로 생성한다.
2. `PropagationProcessor`가 성분별 지연·감쇠·도플러를 적용해 재사용
   `HydrophoneFrame`에 수신점 µPa를 누적한다.
3. `ReceiverArray`가 µPa를 full-scale로 교정하고 주변 소음과 배열 응답을 적용한다.
4. `DspEngine`은 위 객체를 순서대로 호출하며 샘플 루프에서 버퍼를 할당하지 않는다.

다음 경계는 BASS 누산과 향후 LOFAR/DEMON 분석을 `Analysis`로 옮겨 수신기 출력을 읽기만
하게 만드는 것이다. 동시에 결정적 블록 덤프를 추가해 각 경계의 수치를 오프라인에서
검증한다. AudioWorklet의 `DspEngine::process()` 블록당 1회 호출 규약은 계속 유지한다.
