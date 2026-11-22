// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { PersonaChooser } from './chooser';
import { TABLES } from './locales';
import { setLanguage } from './strings';

describe('PersonaChooser', () => {
  let root: HTMLElement; let ch: PersonaChooser;
  beforeEach(() => {
    setLanguage('de');
    document.body.innerHTML = '<div id="r"></div>'; root = document.getElementById('r')!; ch = new PersonaChooser(root);
  });
  const btn = (p: string) => root.querySelector<HTMLButtonElement>(`.persona[data-persona="${p}"]`)!;

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
  it('arrow keys move focus between the two', () => {
    ch.show(vi.fn()); btn('angel').focus();
    btn('angel').dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }));
    expect(document.activeElement).toBe(btn('daemon'));
  });
  it('show() after busy re-enables', () => { ch.show(vi.fn()); ch.setBusy('daemon'); ch.show(vi.fn()); expect(btn('daemon').disabled).toBe(false); });
  it('busy label on the chosen one, reset by setBusy(false)', () => {
    ch.show(vi.fn()); ch.setBusy('daemon');
    expect(btn('daemon').querySelector('.persona-label')!.textContent).toBe(TABLES.de!.start.busy);
    ch.setBusy(false);
    expect(btn('daemon').querySelector('.persona-label')!.textContent).toBe(TABLES.de!.chooser.daemon);
    expect(root.querySelector('.chooser')!.classList.contains('is-busy')).toBe(false);
  });
});
