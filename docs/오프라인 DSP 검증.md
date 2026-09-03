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
| `reference_scene.trace.csv` | 매 샘플의 Source 1 m 음압, 첫 하이드로폰 음압, 제한 전 수신기 FS, 제한 후 출력 FS |
| `reference_scene.summary.json` | 샘플 수, RMS, 절대 피크, 평균, 영교차 수, float 바이트 FNV-1a 해시 |

CSV의 `source_1m_upa`는 현재 수신기 시각에 모든 Source가 방사한 전파 전 음압 합이다.
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
- `cargo test`는 실시간/trace 출력의 샘플별 동일성과 기준 장면 블록 통계를 검사한다.

다음 확장은 trace 블록의 FFT/PSD, 토널 피크, DEMON 변조율과 대역 에너지를 계산해
문헌 기반 소스 프로파일별 골든 기준과 비교하는 것이다.
