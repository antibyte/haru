import { describe, expect, it } from 'vitest';
import { MIN_DELAY_MS, Presence, QUIET_MS, type PresenceStore } from './presence';

function memStore(initial: string | null = null): PresenceStore & { value: string | null } {
  const s = { value: initial, get: () => s.value, set: (v: string) => { s.value = v; } };
  return s;
}

const SPEECH = 0.1;
const QUIET = 0.001;

describe('Presence', () => {
  it('arms on the first begin when the draw is 1, and does not fire before the delay', () => {
    const p = new Presence(memStore(), () => 0);
    expect(p.begin(0)).toBe(true);
    for (let t = 0; t < MIN_DELAY_MS; t += 100) expect(p.tick(t, QUIET, QUIET)).toBe(false);
  });

  it('fires after the delay only once 1.2 s of quiet has passed, and resets when speech returns', () => {
    const p = new Presence(memStore(), () => 0);
    p.begin(0);
    expect(p.tick(MIN_DELAY_MS, QUIET, QUIET)).toBe(false); // quiet just started
    expect(p.tick(MIN_DELAY_MS + QUIET_MS - 1, QUIET, QUIET)).toBe(false);
    expect(p.tick(MIN_DELAY_MS + QUIET_MS, QUIET, SPEECH)).toBe(false); // the user speaks: quiet resets
    expect(p.tick(MIN_DELAY_MS + QUIET_MS + 10, QUIET, QUIET)).toBe(false);
    expect(p.tick(MIN_DELAY_MS + 2 * QUIET_MS + 9, QUIET, QUIET)).toBe(false);
    expect(p.tick(MIN_DELAY_MS + 2 * QUIET_MS + 10, QUIET, QUIET)).toBe(true);
  });

  it('does not fire while the model speaks', () => {
    const p = new Presence(memStore(), () => 0);
    p.begin(0);
    for (let t = MIN_DELAY_MS; t < MIN_DELAY_MS + 10_000; t += 50) expect(p.tick(t, SPEECH, QUIET)).toBe(false);
  });

  it('fires exactly once, then redraws and saves the counter', () => {
    const store = memStore();
    const p = new Presence(store, () => 0);
    p.begin(0);
    p.tick(MIN_DELAY_MS, QUIET, QUIET);
    expect(p.tick(MIN_DELAY_MS + QUIET_MS, QUIET, QUIET)).toBe(true);
    for (let t = MIN_DELAY_MS + 2 * QUIET_MS; t < MIN_DELAY_MS + 20_000; t += 100) expect(p.tick(t, QUIET, QUIET)).toBe(false);
    expect(JSON.parse(store.value!)).toEqual({ remaining: 1, armed: false });
    // the next daemon conversation counts the redrawn 1 down and arms again
    expect(p.begin(100_000)).toBe(true);
  });

  it('with a draw of 5, four conversations do not arm and the fifth does', () => {
    const store = memStore();
    const p = new Presence(store, () => 0.99);
    for (let i = 0; i < 4; i++) {
      expect(p.begin(i)).toBe(false);
      p.end();
    }
    expect(JSON.parse(store.value!)).toEqual({ remaining: 1, armed: false });
    expect(p.begin(5)).toBe(true);
  });

  it('an unarmed conversation never fires, even when forced is not set', () => {
    const p = new Presence(memStore(), () => 0.99);
    p.begin(0);
    for (let t = 0; t < 400_000; t += 100) expect(p.tick(t, QUIET, QUIET)).toBe(false);
  });

  it('an armed moment carries over when the conversation ends early, without decrementing', () => {
    const store = memStore();
    const p = new Presence(store, () => 0);
    expect(p.begin(0)).toBe(true);
    p.end();
    expect(p.tick(MIN_DELAY_MS * 2, QUIET, QUIET)).toBe(false); // not active any more
    const reloaded = new Presence(store, () => 0);
    expect(reloaded.begin(0)).toBe(true);
    expect(JSON.parse(store.value!)).toEqual({ remaining: 0, armed: true });
  });

  it('a corrupt or out-of-range store gives a fresh state', () => {
    for (const bad of ['not json', '{"remaining":9,"armed":true}', '{"remaining":-1}', '{"remaining":1.5}', '{"armed":true}', '[]', '']) {
      const p = new Presence(memStore(bad), () => 0.99); // fresh draw: 5, unarmed
      expect(p.begin(0)).toBe(false);
    }
  });

  it('force() fires on the next tick of an active conversation, even unarmed and not quiet', () => {
    const p = new Presence(memStore(), () => 0.99);
    p.begin(0);
    p.force();
    expect(p.tick(10, SPEECH, SPEECH)).toBe(true);
    expect(p.tick(20, SPEECH, SPEECH)).toBe(false);
  });

  it('force() does nothing before a conversation has begun', () => {
    const p = new Presence(memStore(), () => 0.99);
    p.force();
    expect(p.tick(10, QUIET, QUIET)).toBe(false);
  });
});
