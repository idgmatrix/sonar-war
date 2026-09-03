/** Rust/WASM의 안정된 상선 profile_code와 데이터 파일 ID 사이의 경계. */
export const MERCHANT_PROFILE_CODES = {
  'merchant-bulker-jomopans-echo': 1,
  'merchant-containership-jomopans-echo': 2,
  'merchant-vehicle-carrier-jomopans-echo': 3,
  'merchant-tanker-jomopans-echo': 4,
} as const;

export type MerchantSourceProfileId = keyof typeof MERCHANT_PROFILE_CODES;
export type SourceProfileId = 'legacy-generic' | MerchantSourceProfileId;

export const MERCHANT_TONAL_OVERLAY_CODES = {
  'overseas-harriette-140rpm': 1,
} as const;

export type MerchantTonalOverlayId = keyof typeof MERCHANT_TONAL_OVERLAY_CODES;

export interface ProfiledTarget {
  bearingDeg: number;
  rangeM: number;
  depthM: number;
  sourceProfileId: MerchantSourceProfileId;
  speedKn: number;
  lengthM: number;
  relativeVelocityMs: number;
}

export interface ProfiledTargetV2 extends ProfiledTarget {
  tonalOverlayId: MerchantTonalOverlayId;
  shaftRpm: number;
  bladeCount: number;
}

/**
 * profile target stride 7:
 * [bearing, range, depth, profile_code, speed_kn, length_m, rel_vel_ms]
 */
export function encodeProfiledTargets(targets: readonly ProfiledTarget[]): Float32Array {
  const encoded = new Float32Array(targets.length * 7);
  targets.forEach((target, index) => {
    encoded.set(
      [
        target.bearingDeg,
        target.rangeM,
        target.depthM,
        MERCHANT_PROFILE_CODES[target.sourceProfileId],
        target.speedKn,
        target.lengthM,
        target.relativeVelocityMs,
      ],
      index * 7,
    );
  });
  return encoded;
}

/**
 * profile target v2 stride 10. v1 뒤에 명시적으로 선택한 측정 톤 운항점을 붙인다:
 * [...v1, tonal_overlay_code, shaft_rpm, blade_count]
 */
export function encodeProfiledTargetsV2(targets: readonly ProfiledTargetV2[]): Float32Array {
  const encoded = new Float32Array(targets.length * 10);
  targets.forEach((target, index) => {
    encoded.set(
      [
        target.bearingDeg,
        target.rangeM,
        target.depthM,
        MERCHANT_PROFILE_CODES[target.sourceProfileId],
        target.speedKn,
        target.lengthM,
        target.relativeVelocityMs,
        MERCHANT_TONAL_OVERLAY_CODES[target.tonalOverlayId],
        target.shaftRpm,
        target.bladeCount,
      ],
      index * 10,
    );
  });
  return encoded;
}
