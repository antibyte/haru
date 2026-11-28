// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { TABLES } from './locales';
import { setLanguage } from './strings';
import { TOPIC_MAX, TopicSheet, battleTopic, pickTopicIndex } from './topicsheet';

const de = TABLES.de!;
let root: HTMLElement;
const input = () => root.querySelector<HTMLInputElement>('.topic-input')!;
const sheet = () => root.querySelector('.topic')!;
const click = (sel: string) => root.querySelector<HTMLButtonElement>(sel)!.click();
/** rand() values in order, then 0. */
const seq = (...v: number[]) => () => v.shift() ?? 0;

beforeEach(() => { setLanguage('de'); document.body.innerHTML = '<div id="r"></div>'; root = document.getElementById('r')!; });

describe('topics', () => {
  it('pickTopicIndex never repeats the current topic and reaches all others', () => {
    const seen = new Set<number>();
    for (let k = 0; k < 1000; k++) seen.add(pickTopicIndex(12, () => k / 1000, 5));
    expect(seen.has(5)).toBe(false);
    expect(seen.size).toBe(11);
  });
  it('pickTopicIndex without avoid uses the whole range', () => {
    expect(pickTopicIndex(12, () => 0, null)).toBe(0);
    expect(pickTopicIndex(12, () => 0.9999, null)).toBe(11);
    expect(pickTopicIndex(12, () => 1, null)).toBe(11);
    expect(pickTopicIndex(1, () => 0.5, 0)).toBe(0);
  });
  it('battleTopic trims, falls back to the suggestion and cuts at 120 code points', () => {
    expect(battleTopic('  Darf man lügen?  ', 'X')).toBe('Darf man lügen?');
    expect(battleTopic('   ', ' Vorschlag ')).toBe('Vorschlag');
    const long = '😈'.repeat(130);
    expect([...battleTopic(long, 'X')]).toHaveLength(TOPIC_MAX);
  });
});

describe('TopicSheet', () => {
  it('open() prefills a random topic, selects it and focuses the field', () => {
    const s = new TopicSheet(root, seq(3 / 12));
    s.open(vi.fn());
    expect(sheet().classList.contains('is-visible')).toBe(true);
    expect(input().value).toBe(de.battle.topics[3]);
    expect(document.activeElement).toBe(input());
    expect(input().selectionStart).toBe(0);
    expect(input().selectionEnd).toBe(de.battle.topics[3].length);
  });
  it('the dice picks a different topic', () => {
    const s = new TopicSheet(root, seq(3 / 12, 3 / 11));
    s.open(vi.fn());
    click('.topic-dice');
    expect(input().value).toBe(de.battle.topics[4]);
    expect(s.suggestion).toBe(de.battle.topics[4]);
  });
  it('Start (or Enter) starts with the typed topic and closes', () => {
    const start = vi.fn();
    new TopicSheet(root, seq(0)).open(start);
    input().value = '  Darf man Socken mit Sandalen tragen?  ';
    click('.topic-start');
    expect(start).toHaveBeenCalledWith('Darf man Socken mit Sandalen tragen?');
    expect(sheet().classList.contains('is-visible')).toBe(false);
  });
  it('an empty field starts with the suggestion, which the placeholder shows', () => {
    const start = vi.fn();
    new TopicSheet(root, seq(7 / 12)).open(start);
    input().value = '';
    expect(input().placeholder).toBe(`Vorschlag: ${de.battle.topics[7]}`);
    click('.topic-start');
    expect(start).toHaveBeenCalledWith(de.battle.topics[7]);
  });
  it('the field takes at most 120 characters', () => {
    new TopicSheet(root).open(vi.fn());
    expect(input().maxLength).toBe(120);
  });
  it('relabel switches an untouched suggestion to the same topic in the new language; an edited field stays', () => {
    const s = new TopicSheet(root, seq(2 / 12));
    s.open(vi.fn());
    setLanguage('en'); s.relabel();
    expect(input().value).toBe(TABLES.en!.battle.topics[2]);
    expect(root.querySelector('.topic-label')!.textContent).toBe(TABLES.en!.battle.topicLabel);
    input().value = 'Mine';
    setLanguage('ja'); s.relabel();
    expect(input().value).toBe('Mine');
    expect(input().placeholder).toBe(`候補：${TABLES.ja.battle.topics[2]}`);
  });
  it('Escape closes without starting and reports the close once', () => {
    const start = vi.fn(); const closed = vi.fn();
    new TopicSheet(root).open(start, closed);
    input().dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(closed).toHaveBeenCalledTimes(1);
    expect(start).not.toHaveBeenCalled();
    expect(sheet().classList.contains('is-visible')).toBe(false);
  });
  it('IME composition neither closes nor submits the sheet', () => {
    const start = vi.fn(); const closed = vi.fn();
    new TopicSheet(root).open(start, closed);
    input().dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', isComposing: true, bubbles: true, cancelable: true }));
    expect(sheet().classList.contains('is-visible')).toBe(true);
    expect(closed).not.toHaveBeenCalled();
    const enter = new KeyboardEvent('keydown', { key: 'Enter', isComposing: true, bubbles: true, cancelable: true });
    input().dispatchEvent(enter);
    expect(enter.defaultPrevented).toBe(true);
    expect(start).not.toHaveBeenCalled();
  });
  it('Tab and Shift+Tab stay inside the sheet', () => {
    new TopicSheet(root).open(vi.fn());
    const key = (el: Element, shiftKey: boolean) => el.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', shiftKey, bubbles: true, cancelable: true }));
    const startBtn = root.querySelector<HTMLButtonElement>('.topic-start')!;
    startBtn.focus(); key(startBtn, false);
    expect(document.activeElement).toBe(input());
    key(input(), true);
    expect(document.activeElement).toBe(startBtn);
    (document.activeElement as HTMLElement).blur();
    key(input(), false); expect(document.activeElement).toBe(input());
    (document.activeElement as HTMLElement).blur();
    key(input(), true); expect(document.activeElement).toBe(startBtn);
  });
  it('cancel closes without starting and reports the close', () => {
    const start = vi.fn(); const closed = vi.fn();
    const s = new TopicSheet(root);
    s.open(start, closed);
    click('.topic-cancel');
    expect(start).not.toHaveBeenCalled();
    expect(closed).toHaveBeenCalledTimes(1);
    expect(s.isOpen).toBe(false);
  });
});
