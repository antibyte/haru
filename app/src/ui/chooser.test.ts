// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { PersonaChooser, placeVs, VS_ABOVE, VS_TITLE_GAP } from './chooser';
import { TABLES } from './locales';
import { setLanguage } from './strings';

const rect = (left: number, top: number, width: number, height: number) =>
  ({ left, top, width, height, right: left + width, bottom: top + height, x: left, y: top, toJSON: () => ({}) }) as DOMRect;
const ROW = rect(0, 308, 1600, 592);
const ANGEL = rect(45, 308, 800, 592);
const DAEMON = rect(845, 308, 709, 592);

describe('PersonaChooser', () => {
  let root: HTMLElement; let ch: PersonaChooser;
  beforeEach(() => {
    setLanguage('de');
    document.body.innerHTML = '<div id="r"></div>'; root = document.getElementById('r')!; ch = new PersonaChooser(root);
  });
  const btn = (p: string) => root.querySelector<HTMLButtonElement>(`.persona[data-persona="${p}"]`)!;
  const vs = () => root.querySelector<HTMLButtonElement>('.chooser-vs')!;

  it('angel left, daemon right, both labelled', () => {
    const all = [...root.querySelectorAll<HTMLButtonElement>('.persona')];
    expect(all.map((b) => b.dataset.persona)).toEqual(['angel', 'daemon']);
    for (const b of all) expect(b.getAttribute('aria-label')).toBeTruthy();
  });
  it('click picks the persona', () => {
    const cb = vi.fn(); ch.show(cb); btn('daemon').click(); expect(cb).toHaveBeenCalledWith('daemon', expect.anything());
  });
  it('busy disables both and marks the chosen one', () => {
    const cb = vi.fn(); ch.show(cb); ch.setBusy('angel');
    expect(btn('angel').disabled && btn('daemon').disabled).toBe(true);
    expect(btn('angel').classList.contains('is-chosen')).toBe(true);
    btn('daemon').click(); expect(cb).not.toHaveBeenCalled();
  });
  it('a held key never starts', () => {
    ch.show(vi.fn());
    const ev = new KeyboardEvent('keydown', { key: 'Enter', repeat: true, cancelable: true });
    btn('angel').dispatchEvent(ev); expect(ev.defaultPrevented).toBe(true);
  });
  it('arrow keys move focus angel → VS → daemon and wrap', () => {
    ch.show(vi.fn()); btn('angel').focus();
    const key = (el: HTMLElement, k: string) => el.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true }));
    key(btn('angel'), 'ArrowRight'); expect(document.activeElement).toBe(vs());
    key(vs(), 'ArrowRight'); expect(document.activeElement).toBe(btn('daemon'));
    key(btn('daemon'), 'ArrowRight'); expect(document.activeElement).toBe(btn('angel'));
    key(btn('angel'), 'ArrowLeft'); expect(document.activeElement).toBe(btn('daemon'));
    key(btn('daemon'), 'ArrowLeft'); expect(document.activeElement).toBe(vs());
  });
  it('show() after busy re-enables', () => { ch.show(vi.fn()); ch.setBusy('daemon'); ch.show(vi.fn()); expect(btn('daemon').disabled).toBe(false); });
  it('busy label on the chosen one, reset by setBusy(false)', () => {
    ch.show(vi.fn()); ch.setBusy('daemon');
    expect(btn('daemon').querySelector('.persona-label')!.textContent).toBe(TABLES.de!.start.busy);
    ch.setBusy(false);
    expect(btn('daemon').querySelector('.persona-label')!.textContent).toBe(TABLES.de!.chooser.daemon);
    expect(root.querySelector('.chooser')!.classList.contains('is-busy')).toBe(false);
  });

  it('the VS sits between angel and daemon in DOM and tab order, labelled "Engel vs. Dämon"', () => {
    const order = [...root.querySelectorAll<HTMLButtonElement>('.chooser-row > button')];
    expect(order).toEqual([btn('angel'), vs(), btn('daemon')]);
    expect(vs().getAttribute('aria-label')!.startsWith('Engel vs. Dämon')).toBe(true);
    expect(vs().querySelector('img')!.getAttribute('src')).toBe('/personas/vs.webp');
  });
  it('the VS is out of the flex flow, so angel and daemon keep their boxes', () => {
    expect(vs().style.position).toBe('absolute');
    expect(vs().classList.contains('persona')).toBe(false);
    expect(root.querySelectorAll('.persona')).toHaveLength(2);
  });
  it('placeVs: on the seam, mostly above the images, below the title', () => {
    const p = placeVs(ROW, ANGEL, DAEMON, 78);
    expect(p.stacked).toBe(false);
    expect(p.x).toBeCloseTo(845, 6);
    expect(p.size).toBeCloseTo(177.6, 6);
    const top = ROW.top + p.y - p.size / 2;
    expect(top).toBeCloseTo(ROW.top - VS_ABOVE * p.size, 6);
    expect(top).toBeGreaterThanOrEqual(78 + VS_TITLE_GAP);
    expect(ROW.top + p.y).toBeLessThan(ROW.top);
  });
  it('placeVs: never above the title', () => {
    const row = rect(0, 200, 2000, 518);
    const p = placeVs(row, rect(150, 200, 900, 518), rect(1050, 200, 620, 518), 150);
    expect(row.top + p.y - p.size / 2).toBeCloseTo(150 + VS_TITLE_GAP, 6);
  });
  it('placeVs: stacked layout puts the VS on the seam between the images', () => {
    const p = placeVs(rect(0, 73, 600, 827), rect(0, 73, 600, 413), rect(0, 486, 600, 413), 65);
    expect(p).toMatchObject({ stacked: true, x: 300, y: 413 });
    expect(p.size).toBeCloseTo(99.12, 6);
  });
  it('layout() applies the placement to the VS', () => {
    vi.spyOn(root.querySelector('.chooser-row')!, 'getBoundingClientRect').mockReturnValue(ROW);
    vi.spyOn(btn('angel'), 'getBoundingClientRect').mockReturnValue(ANGEL);
    vi.spyOn(btn('daemon'), 'getBoundingClientRect').mockReturnValue(DAEMON);
    vi.spyOn(root.querySelector('.chooser-title')!, 'getBoundingClientRect').mockReturnValue(rect(600, 44, 400, 34));
    ch.layout();
    expect(parseFloat(vs().style.left)).toBeCloseTo(845, 3);
    expect(parseFloat(vs().style.width)).toBeCloseTo(177.6, 3);
    expect(parseFloat(vs().style.top)).toBeCloseTo(308 - VS_ABOVE * 177.6 + 88.8 - 308, 3);
  });
  it('without battle support the VS is dimmed with the hint and a click does nothing', () => {
    const onBattle = vi.fn(); ch.show(vi.fn(), onBattle); ch.setBattleAvailable(false);
    expect(vs().classList.contains('is-unavailable')).toBe(true);
    expect(vs().getAttribute('aria-disabled')).toBe('true');
    expect(vs().querySelector('.chooser-vs-hint')!.textContent).toBe(TABLES.de!.battle.unsupported);
    expect(vs().title).toBe(TABLES.de!.battle.unsupported);
    vs().click(); expect(onBattle).not.toHaveBeenCalled();
  });
  it('with battle support a click calls onBattle', () => {
    const onBattle = vi.fn(); ch.show(vi.fn(), onBattle); ch.setBattleAvailable(true);
    expect(vs().getAttribute('aria-disabled')).toBe('false');
    vs().click(); expect(onBattle).toHaveBeenCalledTimes(1);
  });
  it('hover and focus make the VS active, only while available, visible and not busy', () => {
    const active = vi.fn(); ch.onVsActive(active); ch.setBattleAvailable(true); ch.show(vi.fn(), vi.fn());
    vs().dispatchEvent(new Event('pointerenter')); expect(active).toHaveBeenLastCalledWith(true);
    vs().dispatchEvent(new Event('pointerleave')); expect(active).toHaveBeenLastCalledWith(false);
    vs().focus(); expect(active).toHaveBeenLastCalledWith(true);
    ch.setBusy('battle'); expect(active).toHaveBeenLastCalledWith(false);
    ch.setBusy(false); expect(active).toHaveBeenLastCalledWith(true);
    ch.setBattleAvailable(false); expect(active).toHaveBeenLastCalledWith(false);
  });
  it('busy("battle") disables all three and marks the VS', () => {
    ch.setBattleAvailable(true); ch.show(vi.fn(), vi.fn()); ch.setBusy('battle');
    expect(btn('angel').disabled && btn('daemon').disabled && vs().disabled).toBe(true);
    expect(vs().classList.contains('is-chosen')).toBe(true);
    expect(vs().querySelector('.chooser-vs-label')!.textContent).toBe(TABLES.de!.start.busy);
  });
});
