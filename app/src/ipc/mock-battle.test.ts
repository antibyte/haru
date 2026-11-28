import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { vi } from 'vitest';
import { MockClient, battleTimeline, type MockOptions } from './mock-client';
import type { VizFrame } from './types';

beforeEach(() => { vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'performance', 'Date'] }); });
afterEach(() => { vi.useRealTimers(); });

async function booted(o: MockOptions = { lang: 'de' }): Promise<MockClient> {
  const m = new MockClient(o);
  const p = m.boot(() => {});
  await vi.advanceTimersByTimeAsync(9000);
  await p;
  return m;
}
async function battle(m: MockClient, topic = 'Darf man lügen?'): Promise<VizFrame[]> {
  const frames: VizFrame[] = [];
  const p = m.startBattle(topic, (f) => frames.push(f));
  await vi.advanceTimersByTimeAsync(700);
  await p;
  return frames;
}
const phases = (fs: VizFrame[]) => fs.map((f) => f.battle!.phase).filter((p, i, a) => i === 0 || a[i - 1] !== p);

describe('mock battle', () => {
  it('boot reports battle support; ?mock=nobattle reports none', async () => {
    expect((await (await booted()).boot(() => {})).battle).toBe(true);
    expect((await (await booted({ lang: 'de', noBattle: true })).boot(() => {})).battle).toBe(false);
  });

  it('Haru opens with "Mephisto," and the topic while Mephisto listens', () => {
    const tl = battleTimeline('de', 'Darf man lügen?');
    expect(tl.slice(0, 7).map((f) => f.angel).join('')).toBe(' Mephisto, Darf man lügen?');
    expect(tl.slice(0, 7).every((f) => f.daemon === '')).toBe(true);
    expect(battleTimeline('ja', '嘘はいいですか').slice(0, 3).map((f) => f.angel).join('')).toBe('メフィスト、');
  });

  it('de, en and ja alternate speakers, with a short aside, and end with a gap', () => {
    for (const lang of ['de', 'en', 'ja'] as const) {
      const tl = battleTimeline(lang, 'X');
      const turns = tl.map((f) => (f.angel.trim() && !f.daemon.trim() ? 'a' : f.daemon.trim() && !f.angel.trim() ? 'd' : ''))
        .filter((s) => s !== '').filter((s, i, a) => s !== a[i - 1]).join('');
      expect(turns.startsWith('adad'), lang).toBe(true);
      expect(tl.some((f) => f.angel.trim() !== '' && f.daemon.trim() !== ''), lang).toBe(true);
      expect(tl.slice(-22).every((f) => f.angel === '' && f.daemon === ''), lang).toBe(true);
    }
  });

  it('preparing: prepared rises for about 3 s, then playing', async () => {
    const m = await booted();
    const frames = await battle(m);
    await vi.advanceTimersByTimeAsync(2000);
    const prep = frames.map((f) => f.battle!);
    expect(prep.every((b) => b.phase === 'preparing')).toBe(true);
    expect(prep.every((b, i) => i === 0 || b.prepared >= prep[i - 1].prepared)).toBe(true);
    expect(prep[prep.length - 1].prepared).toBeGreaterThan(0.6);
    expect(frames.every((f) => f.words.length === 0)).toBe(true);
    await vi.advanceTimersByTimeAsync(1500);
    expect(frames[frames.length - 1].battle).toMatchObject({ phase: 'playing', prepared: 1 });
    await m.stopConversation();
  });

  it('playing frames carry speaker tags and per-speaker levels; mic and latency are zero', async () => {
    const m = await booted();
    const frames = await battle(m);
    await vi.advanceTimersByTimeAsync(20000);
    const words = frames.flatMap((f) => f.words).filter((w) => w.text.trim() !== '');
    expect(words[0]).toMatchObject({ speaker: 'angel', text: ' Me' });
    expect(words.some((w) => w.speaker === 'daemon')).toBe(true);
    expect(frames.some((f) => f.battle!.angel.rms > 0.1 && f.battle!.daemon.rms === 0.01)).toBe(true);
    expect(frames.some((f) => f.battle!.daemon.rms > 0.1 && f.battle!.angel.rms === 0.01)).toBe(true);
    for (const f of frames) {
      expect(f.model.rms).toBe(Math.max(f.battle!.angel.rms, f.battle!.daemon.rms));
      expect(f.mic.rms).toBe(0);
      expect(f.latencyMs).toBe(0);
    }
    await m.stopConversation();
  });

  it('?mock=lag: playing pauses and resumes', async () => {
    const m = await booted({ lang: 'de', fail: 'lag' });
    const frames = await battle(m);
    await vi.advanceTimersByTimeAsync(3500 + 12000 + 3500);
    expect(phases(frames)).toEqual(['preparing', 'playing', 'paused', 'playing']);
    const paused = frames.filter((f) => f.battle!.phase === 'paused');
    expect(paused.every((f) => f.words.length === 0)).toBe(true);
    await m.stopConversation();
  });

  it('refuses without battle support and with an empty topic', async () => {
    await expect((await booted({ lang: 'de', noBattle: true })).startBattle('X', () => {})).rejects.toMatchObject({ key: 'battleUnsupported' });
    await expect((await booted()).startBattle('   ', () => {})).rejects.toMatchObject({ key: 'battleTopic' });
  });

  it('stopConversation ends a battle', async () => {
    const m = await booted();
    const frames = await battle(m);
    expect((await m.getStatus()).state).toBe('talking');
    expect(m.lastTopic).toBe('Darf man lügen?');
    await m.stopConversation();
    const n = frames.length;
    await vi.advanceTimersByTimeAsync(1000);
    expect(frames.length).toBe(n);
    expect((await m.getStatus()).state).toBe('ready');
  });
});
