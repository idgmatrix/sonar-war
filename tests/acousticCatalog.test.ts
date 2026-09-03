import { readFileSync } from 'node:fs';
import Ajv2020 from 'ajv/dist/2020.js';
import addFormats from 'ajv-formats';
import { describe, expect, it } from 'vitest';

type Confidence = 'A' | 'B' | 'C';

interface Parameter {
  id: string;
  value: number | { min: number; max: number };
  unit: string;
  reference_conditions: string;
  confidence: Confidence;
  uncertainty: string;
  evidence_refs: string[];
}

interface Catalog {
  schema_version: string;
  level_convention: Record<string, string>;
  layer_contract: { source_includes: string[]; source_excludes: string[] };
  references: Array<{ id: string; url: string; accessed_on: string }>;
  sources: Array<{
    id: string;
    priority: 'P0' | 'P1' | 'P2' | 'P3';
    status: string;
    confidence: Confidence;
    components: Array<{ parameters: Parameter[] }>;
    evidence_refs: string[];
    limitations: string[];
  }>;
}

interface MerchantProfiles {
  model: {
    evidence_ref: string;
    frequency_range_hz: { min: number; max: number };
    uncertainty_std_db: number;
    coefficients: Record<string, number>;
  };
  profiles: Array<{
    id: string;
    vessel_class: string;
    reference_speed_kn: number;
    cargo_low_frequency_damping: number;
    speed_model: { doubling_delta_db: number; valid_range: string };
    directionality: { mode: string; correction_db: number; confidence: Confidence };
  }>;
  verification_case: {
    profile_id: string;
    speed_kn: number;
    length_m: number;
    anchors: Array<{
      frequency_hz: number;
      spectrum_level_db: number;
      decidecade_level_db: number;
    }>;
  };
}

const catalog = JSON.parse(
  readFileSync(new URL('../data/acoustics/catalog.json', import.meta.url), 'utf8'),
) as Catalog;
const schema = JSON.parse(
  readFileSync(new URL('../data/acoustics/schema/catalog.schema.json', import.meta.url), 'utf8'),
) as object;
const merchantProfiles = JSON.parse(
  readFileSync(new URL('../data/acoustics/merchant-profiles.json', import.meta.url), 'utf8'),
) as MerchantProfiles;
const merchantSchema = JSON.parse(
  readFileSync(
    new URL('../data/acoustics/schema/merchant-profiles.schema.json', import.meta.url),
    'utf8',
  ),
) as object;

describe('음향 소스 카탈로그', () => {
  it('Draft 2020-12 JSON Schema를 통과한다', () => {
    const validator = new Ajv2020({ allErrors: true });
    addFormats(validator);
    const validate = validator.compile(schema);
    const valid = validate(catalog);
    expect(validate.errors, JSON.stringify(validate.errors, null, 2)).toBeNull();
    expect(valid).toBe(true);
  });

  it('고유한 소스/출처 ID와 고정 레벨 규약을 가진다', () => {
    expect(catalog.schema_version).toBe('1.0.0');
    expect(catalog.level_convention).toEqual({
      source_level: 'dB re 1 µPa @ 1 m',
      spectral_density: 'dB re 1 µPa²/Hz',
      full_scale: '1 Pa = 1,000,000 µPa = 1.0 FS (120 dB re 1 µPa)',
    });

    const sourceIds = catalog.sources.map((source) => source.id);
    const referenceIds = catalog.references.map((reference) => reference.id);
    expect(new Set(sourceIds).size).toBe(sourceIds.length);
    expect(new Set(referenceIds).size).toBe(referenceIds.length);
  });

  it('발생원 계층에서 전파와 수신기 효과를 제외한다', () => {
    expect(catalog.layer_contract.source_includes).toContain('emission_at_reference_distance');
    expect(catalog.layer_contract.source_excludes).toEqual(
      expect.arrayContaining(['transmission_loss', 'doppler', 'receiver_array_response']),
    );
  });

  it('모든 수치 파라미터에 단위·조건·불확실성·유효한 출처가 있다', () => {
    const referenceIds = new Set(catalog.references.map((reference) => reference.id));

    for (const source of catalog.sources) {
      for (const ref of source.evidence_refs) expect(referenceIds.has(ref)).toBe(true);
      expect(source.limitations.length).toBeGreaterThan(0);

      for (const component of source.components) {
        for (const parameter of component.parameters) {
          expect(parameter.unit.length).toBeGreaterThan(0);
          expect(parameter.reference_conditions.length).toBeGreaterThan(0);
          expect(parameter.uncertainty.length).toBeGreaterThan(0);
          expect(['A', 'B', 'C']).toContain(parameter.confidence);
          expect(parameter.evidence_refs.length).toBeGreaterThan(0);
          for (const ref of parameter.evidence_refs) expect(referenceIds.has(ref)).toBe(true);

          if (typeof parameter.value !== 'number') {
            expect(parameter.value.max).toBeGreaterThanOrEqual(parameter.value.min);
          }
        }
      }
    }
  });

  it('P0 목록과 현재 근거 부족 상태를 명시한다', () => {
    const p0Ids = catalog.sources
      .filter((source) => source.priority === 'P0')
      .map((source) => source.id);
    expect(p0Ids).toEqual(
      expect.arrayContaining([
        'large-merchant-vessel',
        'submarine-generic',
        'torpedo-generic',
        'ocean-surface-ambient',
      ]),
    );

    for (const id of ['submarine-generic', 'torpedo-generic']) {
      const source = catalog.sources.find((candidate) => candidate.id === id)!;
      expect(source.status).toBe('research_required');
      expect(source.confidence).toBe('C');
    }
  });
});

describe('JOMOPANS-ECHO 상선 프로파일', () => {
  it('전용 JSON Schema를 통과하고 네 화물선 계열을 구분한다', () => {
    const validator = new Ajv2020({ allErrors: true });
    addFormats(validator);
    const validate = validator.compile(merchantSchema);
    expect(validate(merchantProfiles), JSON.stringify(validate.errors, null, 2)).toBe(true);

    expect(merchantProfiles.profiles.map((profile) => profile.vessel_class)).toEqual([
      'bulker',
      'containership',
      'vehicle_carrier',
      'tanker',
    ]);
    expect(merchantProfiles.model.frequency_range_hz).toEqual({ min: 20, max: 20_000 });
    expect(merchantProfiles.model.uncertainty_std_db).toBe(6);
  });

  it('방향성 미확정값과 속력 외삽 한계를 숨기지 않는다', () => {
    const referenceIds = new Set(catalog.references.map((reference) => reference.id));
    expect(referenceIds.has(merchantProfiles.model.evidence_ref)).toBe(true);

    for (const profile of merchantProfiles.profiles) {
      expect(profile.directionality).toMatchObject({
        mode: 'isotropic_model_baseline',
        correction_db: 0,
        confidence: 'C',
      });
      expect(profile.speed_model.valid_range).toContain('avoid unbounded extrapolation');
      expect(profile.speed_model.doubling_delta_db).toBeCloseTo(60 * Math.log10(2), 4);
    }
  });

  it('공식 보조 계산기의 벌크선 스펙트럼 앵커를 재현한다', () => {
    const coefficients = merchantProfiles.model.coefficients;
    const verification = merchantProfiles.verification_case;
    const profile = merchantProfiles.profiles.find(
      (candidate) => candidate.id === verification.profile_id,
    )!;

    for (const anchor of verification.anchors) {
      const lowFrequency = anchor.frequency_hz < coefficients.cargo_low_frequency_limit_hz;
      const exponent = lowFrequency ? 2 : 0;
      const k = lowFrequency
        ? coefficients.cargo_low_frequency_k_db
        : coefficients.high_frequency_k_db;
      const damping = lowFrequency
        ? profile.cargo_low_frequency_damping
        : coefficients.high_frequency_damping;
      const f1 =
        (lowFrequency
          ? coefficients.cargo_low_frequency_f1_numerator_hz_kn
          : coefficients.high_frequency_f1_numerator_hz_kn) / profile.reference_speed_kn;
      const frequencyPower = 0.5 * (exponent + 2);
      const spectrumLevel =
        k -
        10 * (exponent + 2) * Math.log10(f1) +
        5 * exponent * Math.log10(anchor.frequency_hz) -
        10 *
          Math.log10(
            (1 - (anchor.frequency_hz / f1) ** frequencyPower) ** 2 + damping ** 2,
          ) +
        coefficients.speed_log10_multiplier_db *
          Math.log10(verification.speed_kn / profile.reference_speed_kn) +
        coefficients.length_log10_multiplier_db *
          Math.log10(verification.length_m / coefficients.reference_length_m);
      const bandLevel = spectrumLevel + 10 * Math.log10(0.231 * anchor.frequency_hz);

      expect(spectrumLevel).toBeCloseTo(anchor.spectrum_level_db, 5);
      expect(bandLevel).toBeCloseTo(anchor.decidecade_level_db, 5);
    }
  });
});
