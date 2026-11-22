import { beforeEach, describe, expect, it } from 'vitest';
import { badgeLabel, formatBytes, formatEta, formatRate } from './format';
import { formatNumber, formatPercent, setLanguage } from './strings';

/** Intl uses no-break spaces (U+00A0, U+202F) in some languages, e.g. de "42 %". */
const plain = (s: string) => s.replace(/[  ]/g, ' ');

describe('format', () => {
  beforeEach(() => { setLanguage('de'); });

  it('formatBytes', () => {
    expect(formatBytes(6623102720)).toBe('6,62 GB');
    expect(formatBytes(536870912)).toBe('536,87 MB');
    expect(formatBytes(512e6)).toBe('512,00 MB');
  });
  it('formatRate', () => {
    expect(formatRate(42100000)).toBe('42,1 MB/s');
  });
  it('formatBytes and formatRate in English', () => {
    setLanguage('en');
    expect(formatBytes(6623102720)).toBe('6.62 GB');
    expect(formatBytes(512e6)).toBe('512.00 MB');
    expect(formatRate(42100000)).toBe('42.1 MB/s');
  });
  it('formatNumber never groups thousands', () => {
    expect(formatNumber(1234.5, 1)).toBe('1234,5');
    setLanguage('en');
    expect(formatNumber(1234.5, 1)).toBe('1234.5');
  });
  it('formatPercent', () => {
    expect(plain(formatPercent(42.4))).toBe('42 %');
    expect(plain(formatPercent(100))).toBe('100 %');
    setLanguage('en');
    expect(formatPercent(42.4)).toBe('42%');
    expect(formatPercent(0)).toBe('0%');
  });
  it('formatEta', () => {
    expect(formatEta(45)).toBe('45 s');
    expect(formatEta(105)).toBe('1 min 45 s');
    expect(formatEta(3725)).toBe('1 h 2 min');
    expect(formatEta(Infinity)).toBe('–');
  });
  it('badgeLabel', () => {
    expect(badgeLabel('vulkan', 'Intel(R) Arc(TM) B580 Graphics')).toBe('Vulkan · Intel Arc B580');
    expect(badgeLabel('cpu', 'CPU')).toBe('CPU');
    expect(badgeLabel('cuda', 'NVIDIA GeForce RTX 4090')).toBe('CUDA · NVIDIA GeForce RTX 4090');
  });
});
