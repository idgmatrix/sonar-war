# 오프라인 DSP 검증

> 목적: 청취에 의존하지 않고 실시간 엔진과 동일한 상태 전이에서 계층별 수치와 최종
> 오디오를 반복 비교한다.

## 실행

```bash
npm run dsp:render -- --out artifacts/offline/reference_scene
```

선택 인자는 `--seconds`, `--warmup-seconds`, `--sample-rate`다. 기본값은 각각 1초,
0.25초, 44,100 Hz이며 기준 장면은 정면 1 km의 120 RPM·5엽 표적이다.

## 산출물

| 파일 | 내용 |
|---|---|
| `reference_scene.wav` | 최종 출력의 mono IEEE float32 WAV. PCM 양자화 없이 FS 값을 보존한다. |
| `reference_scene.trace.csv` | 매 샘플의 Source 1 m 전체·토널·광대역 음압, 첫 하이드로폰 음압, 제한 전 수신기 FS, 제한 후 출력 FS |
| `reference_scene.summary.json` | 블록 통계·해시와 Source 토널/DEMON 피크 주파수·PSD |

CSV의 `source_1m_upa`는 현재 수신기 시각에 모든 Source가 방사한 전파 전 음압 합이며,
`source_tonal_1m_upa`와 `source_broadband_1m_upa`로 성분을 분리해 함께 기록한다.
`hydrophone_0_upa`는 TL·도플러·배열 도달 지연을 적용한 첫 하이드로폰 수신 음압이다.
`receiver_fs`에는 수신기 교정·빔포밍·주변 소음이 적용되고, `output_fs`에는 마지막
`tanh` 제한까지 적용된다.

## 결정성과 골든 기준

- `process_traced`는 실시간 `process`와 같은 내부 샘플 함수를 호출한다. trace 기록은
  Source 상태를 읽기만 하며 난수나 시간축을 추가로 전진시키지 않는다.
- 같은 실행 환경과 툴체인에서 같은 입력의 WAV·CSV·JSON은 바이트 단위로 같아야 한다.
- FNV-1a는 회귀 식별자이지 암호학적 해시가 아니다. 플랫폼 수학 함수 차이를 고려해
  교차 플랫폼 합격 판정은 RMS·피크·영교차의 문서화된 허용 오차를 사용한다.
- 기준값과 허용 오차는 `data/acoustics/golden/dsp_reference_scene.csv`에 둔다.
- `cargo test`는 실시간/trace 출력의 샘플별 동일성, 기준 장면 블록 통계, 토널과
  DEMON 피크 주파수를 검사한다.

## 스펙트럼 분석 규약

- 실제 샘플 길이에 Hann 윈도우를 적용하고 다음 2의 거듭제곱까지 영 패딩한다.
- 단측 PSD는 입력 단위²/Hz이며 Hann 윈도우 전력으로 정규화한다.
- 피크 주파수는 최대 빈 주변 로그 PSD 세 점의 포물선 보간으로 구한다.
- 협대역 선 전체 준위는 알려진 목표 주파수에서 Hann 직교 검파한 피크 진폭을 RMS로
  변환해 측정한다. 따라서 FFT 빈 폭에 따라 달라지는 `dB/Hz` 피크와 혼동하지 않는다.
- DEMON은 분리된 광대역 Source 압력을 제곱해 포락선을 얻고 평균 전력을 제거한 뒤
  같은 PSD 절차를 적용한다. 기준 장면의 120 RPM·5엽 블레이드 레이트는 10 Hz다.
- 영 패딩은 피크 보간을 돕지만 실제 분해능을 늘리지 않으므로 허용 오차는 관측 길이와
  창 함수의 영향을 포함해 정한다.

`overseas_harriette_140rpm_tones_1km.csv`는 15개 측정 톤 각각의 Source RMS 준위와
1 km 구면 확산·Thorp 흡수 후 준위를 기록한다. 8초 순수 톤 합성을 통해 주파수
`±0.04 Hz`, 원시·전파 후 준위 `±0.05 dB`를 자동 검사한다. JOMOPANS 광대역과
수신기 잡음은 각각의 기존 골든에서 별도로 검증한다.

다음 확장은 대역 적분 에너지와 채널 임펄스 응답을 문헌 기반 소스 프로파일별 골든
기준과 비교하는 것이다.
