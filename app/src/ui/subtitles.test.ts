// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { WordEvent } from '../words/assembler';
import { Subtitles } from './subtitles';

let root: HTMLElement; let sub: Subtitles;
beforeEach(() => {
  vi.useFakeTimers();
  document.body.innerHTML = '<div id="r"></div>'; root = document.getElementById('r')!;
  sub = new Subtitles(root); sub.setEnabled(true);
});
afterEach(() => { vi.useRealTimers(); });
const say = (id: number, text: string, speaker?: WordEvent['speaker']) =>
  sub.handle({ type: 'wordStart', id, text, seq: id, ...(speaker ? { speaker } : {}) });
const lines = () => [...root.querySelectorAll('.subtitle-line')].map((l) => `${l.className}|${l.textContent}`);

describe('Subtitles in a battle', () => {
  it('each speaker gets its own colour class', () => {
    say(1, 'Ja', 'angel'); say(2, 'Nein', 'daemon');
    expect(lines()).toEqual(['subtitle-line is-angel|Ja', 'subtitle-line is-daemon|Nein']);
  });
  it('a speaker change starts a new line, the same speaker continues it', () => {
    say(1, 'Ehrlichkeit', 'angel'); say(3, 'macht', 'angel'); say(2, 'Pah.', 'daemon'); say(5, 'frei.', 'angel');
    expect(lines()).toEqual(['subtitle-line is-daemon|Pah.', 'subtitle-line is-angel|frei.']);
  });
  it('conversation words without a speaker stay untagged on one line', () => {
    say(1, 'Hallo'); say(2, 'du');
    expect(lines()).toEqual(['subtitle-line|Hallo du']);
  });
});
