import { describe, expect, it } from 'vitest';
import { WordAssembler } from './assembler';

const code = { wordStart: 'S', wordAppend: 'A', wordEnd: 'E' } as const;

/** Runs frames and renders each frame's events as "S1:Hal" strings. */
function run(frames: string[], flush = false): string[][] {
  const a = new WordAssembler();
  const out = frames.map((text, seq) => a.push({ seq, text }).map((e) => `${code[e.type]}${e.id}:${e.text}`));
  if (flush) out.push(a.flush().map((e) => `${code[e.type]}${e.id}:${e.text}`));
  return out;
}

describe('WordAssembler', () => {
  it('german_subwords', () => {
    expect(run([' Hal', 'lo', ' Wel', 't', '.'])).toEqual([
      ['S1:Hal'], ['A1:Hallo'], ['E1:Hallo', 'S2:Wel'], ['A2:Welt'], ['A2:Welt.', 'E2:Welt.'],
    ]);
  });
  it('multiple_words_in_one_frame', () => {
    expect(run([' ich bin', ' da'])).toEqual([['S1:ich', 'E1:ich', 'S2:bin'], ['E2:bin', 'S3:da']]);
  });
  it('silence_ends_word', () => {
    expect(run([' Ja', ' ', ' '])).toEqual([['S1:Ja'], ['E1:Ja'], []]);
  });
  it('empty_frames_do_not_split_latin', () => {
    expect(run([' Ver', '', '', 'stehe'])).toEqual([['S1:Ver'], [], [], ['A1:Verstehe']]);
  });
  it('latin_idle_timeout', () => {
    const r = run([' Gut', '', '', '', '', '', '']);
    expect(r.slice(1, 6).every((e) => e.length === 0)).toBe(true);
    expect(r[6]).toEqual(['E1:Gut']);
  });
  it('punctuation_mid_piece', () => {
    expect(run([' Ja,', 'und', ' so'])).toEqual([['S1:Ja,', 'E1:Ja,'], ['S2:und'], ['E2:und', 'S3:so']]);
  });
  it('standalone_punctuation_dropped', () => {
    expect(run([' .'])).toEqual([[]]);
  });
  it('cjk_punctuation', () => {
    expect(run(['こんに', 'ちは', '。'])).toEqual([['S1:こんに'], ['A1:こんにちは'], ['A1:こんにちは。', 'E1:こんにちは。']]);
  });
  it('cjk_two_empty_frames', () => {
    expect(run(['今日', '', ''])).toEqual([['S1:今日'], [], ['E1:今日']]);
  });
  it('cjk_eight_char_cap', () => {
    expect(run(['一二三四五六七八九十'])).toEqual([['S1:一二三四五六七八', 'E1:一二三四五六七八', 'S2:九十']]);
  });
  it('script_change_ends_word', () => {
    expect(run(['日本', 'Tokyo'])).toEqual([['S1:日本'], ['E1:日本', 'S2:Tokyo']]);
  });
  it('korean_uses_spaces', () => {
    expect(run([' 안녕', '하세요', ' 반가워요'])).toEqual([['S1:안녕'], ['A1:안녕하세요'], ['E1:안녕하세요', 'S2:반가워요']]);
  });
  it('flush_ends_open_word', () => {
    expect(run([' Hal'], true)).toEqual([['S1:Hal'], ['E1:Hal']]);
  });
  it('ids_stay_monotonic_after_reset', () => {
    const a = new WordAssembler();
    a.push({ seq: 0, text: ' Hi' });
    a.reset();
    expect(a.push({ seq: 1, text: ' Ho' })[0].id).toBe(2);
  });
});
