import { describe, expect, it } from 'vitest';
import { badgeLabel, formatBytes, formatEta, formatRate } from './format';

describe('format', () => {
  it('formatBytes', () => {
    expect(formatBytes(6623102720)).toBe('6,62 GB');
    expect(formatBytes(536870912)).toBe('536,87 MB');
    expect(formatBytes(512e6)).toBe('512,00 MB');
  });
  it('formatRate', () => {
    expect(formatRate(42100000)).toBe('42,1 MB/s');
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
