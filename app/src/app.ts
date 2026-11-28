import { Renderer } from './render/renderer';
import type { BootInfo, Persona, HaruClient, HaruError, Settings, StateEvent, UiText, VizFrame } from './ipc/types';
import { toHaruError } from './ipc/tauri-client';
import { BootScreen } from './ui/boot';
import { HeadphoneHint } from './ui/hint';
import { PersonaChooser } from './ui/chooser';
import { Hud, type HudAction } from './ui/hud';
import { SettingsPanel } from './ui/settings';
import { Subtitles } from './ui/subtitles';
import { ErrorToast } from './ui/toast';
import { bindKeys } from './ui/keys';
import { badgeLabel } from './ui/format';
import { Presence, localPresenceStore } from './ui/presence';
import { PreparingOverlay } from './ui/preparing';
import { TopicSheet } from './ui/topicsheet';
import { arrowsFor, prefersReducedMotion } from './ui/arrows';
import { getLanguage, resolveLanguage, sameText, setLanguage } from './ui/strings';

/** How long the moment's picture stays up. */
const MOMENT_MS = 500;
/** Debug (`?debug&moment`): the moment fires this long after a daemon conversation starts. */
const MOMENT_FORCE_DELAY_MS = 1500;

export interface AppHooks { autostart: boolean; log: (msg: string) => void }

type Phase = 'boot' | 'idle' | 'starting' | 'talking';

/** Wires client, renderer and overlays together. */
export class App {
  private readonly renderer: Renderer;
  private readonly boot: BootScreen;
  private readonly hint: HeadphoneHint;
  private readonly chooser: PersonaChooser;
  private readonly hud: Hud;
  private readonly settingsPanel: SettingsPanel;
  private readonly subtitles: Subtitles;
  private readonly toast: ErrorToast;
  private readonly topicSheet: TopicSheet;
  private readonly preparing: PreparingOverlay;

  private settings: Settings | null = null;
  private info: BootInfo | null = null;
  private phase: Phase = 'boot';
  private booting = false;
  private noAutostart = false;
  private errorShown = false;
  private muted = false;
  private lastWarning: UiText | null = null;
  private readonly presence = new Presence(localPresenceStore());
  /** true while the current conversation is a daemon one and feeds the presence scheduler */
  private presenceActive = false;
  private forceTimer: ReturnType<typeof setTimeout> | null = null;
  /** true while a battle starts or runs (no microphone, no conversation persona; battles do not count as daemon conversations) */
  private battle = false;

  constructor(private readonly client: HaruClient, private readonly canvas: HTMLCanvasElement, root: HTMLElement, private readonly hooks?: AppHooks) {
    this.renderer = new Renderer(this.canvas);
    this.boot = new BootScreen(root);
    this.hint = new HeadphoneHint(root);
    this.chooser = new PersonaChooser(root);
    this.preparing = new PreparingOverlay(root);
    this.topicSheet = new TopicSheet(root);
    this.hud = new Hud(root);
    this.settingsPanel = new SettingsPanel(root);
    this.subtitles = new Subtitles(root);
    this.toast = new ErrorToast(root);
    this.renderer.onWordEvent((e) => this.subtitles.handle(e));
    this.hud.onAction((a) => void this.onHudAction(a));
    this.settingsPanel.onSave((s) => this.saveSettings(s));
    this.chooser.onVsActive((on) => this.onVsActive(on));
    bindKeys({
      mute: () => { if (this.phase === 'talking' && !this.battle) void this.toggleMute(); },
      settings: () => void this.toggleSettings(),
      fullscreen: () => void this.toggleFullscreen(),
      escape: () => { this.settingsPanel.close(); this.topicSheet.close(); },
    });
  }

  async start(): Promise<void> {
    this.renderer.start();
    try {
      this.applySettings(await this.client.getSettings());
    } catch { /* defaults are fine until boot reports the real problem */ }
    await this.client.onState((e) => this.onState(e));
    try {
      // A reloaded page cannot re-attach to a running conversation (its viz channel is gone): end it.
      if ((await this.client.getStatus()).state === 'talking') {
        this.hooks?.log('start: core still talking after reload, stopping the old conversation');
        this.noAutostart = true;
        await this.client.stopConversation();
      }
    } catch { /* boot below reports any real problem */ }
    await this.runBoot();
  }

  // ---------------------------------------------------------------- state

  private onState(e: StateEvent): void {
    this.hooks?.log(`state ${e.state}${e.error ? ' error=' + e.error.code : ''}${e.warning ? ' warning=' + e.warning.key : ''}${e.notice ? ' notice=' + e.notice.key : ''}`);
    if (e.notice && e.state === 'talking' && this.phase === 'talking') this.toast.showInfo(e.notice);
    // compare key + params: the same warning arrives with every state event and must not pop up again
    if (e.warning && !sameText(e.warning, this.lastWarning)) {
      this.lastWarning = e.warning;
      this.toast.showWarning(e.warning);
    }
    switch (e.state) {
      case 'booting':
      case 'downloading':
      case 'loading':
        if (this.phase === 'starting' || this.phase === 'talking') return; // backend fallback during start
        if (!this.booting) void this.runBoot();
        this.boot.setStatus(e.state === 'downloading' ? 'downloading' : e.state === 'loading' ? 'loading' : 'detecting');
        break;
      case 'error':
        if (e.error) this.showError(e.error);
        this.boot.hide();
        this.endTalk();
        break;
      case 'ready':
        if (e.error && (this.phase === 'talking' || this.phase === 'starting')) {
          // the conversation ended by itself (lag limit, worker error, dead microphone)
          this.toast.show(e.error);
          this.endTalk();
          if (!this.booting) this.showStart();
        } else if (e.error) {
          this.toast.show(e.error);
        } else if (this.battle && this.phase === 'talking') {
          this.endTalk(); // the battle ended by itself (20 minutes): back to the chooser
          if (!this.booting) this.showStart();
        } else if (this.phase === 'boot' && !this.booting) {
          this.showStart(); // safety net: the core is ready but the UI never got to the Start screen
        }
        break;
      case 'talking':
        break;
    }
  }

  private showError(err: HaruError): void {
    this.errorShown = true;
    this.toast.show(err, () => void this.runBoot());
  }

  private async runBoot(): Promise<void> {
    if (this.booting) return;
    this.booting = true;
    this.errorShown = false;
    this.phase = 'boot';
    this.toast.hide();
    this.chooser.hide();
    this.boot.setStatus('detecting');
    this.boot.show();
    this.renderer.setMode('boot');
    try {
      this.info = await this.client.boot((ev) => {
        this.boot.onDownload(ev);
        if (ev.event === 'progress') this.renderer.setDownloadRate(ev.data.bytesPerSec);
        else this.renderer.setDownloadRate(0);
      });
    } catch (e) {
      this.booting = false;
      this.boot.hide();
      this.renderer.setMode('idle');
      if (!this.errorShown) this.showError(toHaruError(e));
      return;
    }
    this.booting = false;
    this.boot.hide();
    this.chooser.setBattleAvailable(this.info?.battle === true);
    this.hud.setBadge(this.info ? badgeLabel(this.info.backend, this.info.deviceName) : '');
    if (this.info?.warning && !sameText(this.info.warning, this.lastWarning)) {
      this.lastWarning = this.info.warning;
      this.toast.showWarning(this.info.warning);
    }
    try {
      this.applySettings(await this.client.getSettings());
    } catch { /* keep the previous settings */ }
    this.phase = 'idle';
    this.renderer.setMode('idle');
    if (this.settings && !this.settings.headphoneHintSeen) {
      const shown = this.hint.show();
      if (this.hooks?.autostart) this.hint.dismiss();
      await shown;
      try {
        const r = await this.client.saveSettings({ ...this.settings, headphoneHintSeen: true });
        this.applySettings(r.settings);
      } catch { /* the hint simply shows again next time */ }
    }
    if (this.phase === 'idle') this.showStart();
    if (this.hooks?.autostart && !this.noAutostart && this.phase === 'idle') {
      this.hooks.autostart = false; // once
      this.hooks.log('autostart: pressing start');
      void this.startConversation('autostart', this.autostartPersona());
    }
  }

  private showStart(): void {
    this.phase = 'idle';
    this.renderer.setMode('idle');
    this.renderer.setPersona(null);
    this.chooser.show((p, ev) => {
      this.hooks?.log(`start click persona=${p} isTrusted=${ev.isTrusted} detail=${ev.detail}`);
      void this.startConversation('click', p);
    }, () => this.openTopicSheet());
  }

  /** `?persona=daemon` picks the devil for the autostart, only while test hooks are active. */
  private autostartPersona(): Persona {
    if (!this.hooks) return 'angel';
    try {
      return new URLSearchParams(window.location.search).get('persona') === 'daemon' ? 'daemon' : 'angel';
    } catch { return 'angel'; }
  }

  // ---------------------------------------------------------------- battle

  private onVsActive(on: boolean): void {
    if (!on || this.topicSheet.isOpen || this.phase !== 'idle') { this.renderer.setArrows(null); return; }
    this.renderer.setArrows(arrowsFor(this.chooser.rects(), this.canvas.getBoundingClientRect(), prefersReducedMotion()));
  }

  private openTopicSheet(): void {
    if (this.phase !== 'idle') return;
    this.renderer.setArrows(null);
    this.topicSheet.open((topic) => void this.startBattle(topic), () => this.chooser.focusVs());
  }

  /** Angel against daemon on `topic`. Never touches the presence scheduler and never passes a conversation persona. */
  private async startBattle(topic: string): Promise<void> {
    if (this.phase !== 'idle') return;
    this.hooks?.log(`startBattle topic=${JSON.stringify(topic)}`);
    this.phase = 'starting';
    this.battle = true;
    this.chooser.setBusy('battle');
    this.renderer.setArrows(null);
    this.renderer.setBattle(true);
    this.toast.hide();
    try {
      await this.client.startBattle(topic, (f) => this.onBattleViz(f));
    } catch (e) {
      const ended = this.phase !== 'starting'; // the UI already left (its own toast was shown)
      this.phase = 'idle';
      this.battle = false;
      this.chooser.setBusy(false);
      this.renderer.setBattle(false);
      this.hooks?.log(`battle start failed: ${JSON.stringify(toHaruError(e))}`);
      if (!ended) this.toast.show(toHaruError(e));
      return;
    }
    if (this.phase !== 'starting') { void this.client.stopConversation(); return; } // ended while starting: stop the battle the core just started
    this.phase = 'talking';
    this.chooser.hide();
    this.hud.setBattle(true);
    this.hud.show();
    this.renderer.setMode('talking'); // both heads start loading now, during `preparing`
    this.preparing.update('preparing', 0);
  }

  private onBattleViz(f: VizFrame): void {
    this.renderer.pushViz(f);
    if (f.battle && this.phase === 'talking') this.preparing.update(f.battle.phase, f.battle.prepared);
  }

  // ---------------------------------------------------------------- conversation

  private async startConversation(source: 'click' | 'autostart', persona: Persona = 'angel'): Promise<void> {
    if (this.phase !== 'idle') return;
    this.hooks?.log(`startConversation source=${source} persona=${persona}`);
    this.phase = 'starting';
    this.chooser.setBusy(persona);
    this.renderer.setPersona(persona);
    this.toast.hide();
    try {
      await this.client.startConversation(persona, (f) => {
        if (this.presenceActive && persona === 'daemon' && this.presence.tick(performance.now(), f.model.rms, f.mic.rms)) this.fireMoment();
        this.renderer.pushViz(f);
        this.hud.setLatency(f.latencyMs);
        this.hud.setMicLevel(f.mic.rms);
      });
    } catch (e) {
      this.phase = 'idle';
      this.chooser.setBusy(false);
      this.renderer.setPersona(null);
      this.hooks?.log(`start failed: ${JSON.stringify(toHaruError(e))}`);
      this.toast.show(toHaruError(e));
      return;
    }
    if (this.phase !== 'starting') return; // ended while starting
    this.phase = 'talking';
    this.chooser.hide();
    this.hud.show();
    this.hud.setMuted(this.muted);
    this.renderer.setMode('talking');
    if (this.muted) void this.client.setMuted(true);
    if (persona === 'daemon') this.beginPresence();
  }

  private beginPresence(): void {
    this.presenceActive = true;
    const armed = this.presence.begin(performance.now());
    const forced = this.debugFlag('moment');
    this.renderer.keepFaceReady(armed || forced); // the head must be loaded when the moment comes, even with the face off
    if (forced) this.forceTimer = setTimeout(() => this.presence.force(), MOMENT_FORCE_DELAY_MS);
  }

  /** The moment: hard cut to the close-up for MOMENT_MS, with the cue on the same frame. Never throws. */
  private fireMoment(): void {
    if (this.renderer.startMoment(MOMENT_MS)) {
      const root = document.documentElement;
      root.classList.add('is-moment');
      setTimeout(() => root.classList.remove('is-moment'), MOMENT_MS);
    }
    this.client.playCue().catch((e) => this.hooks?.log(`cue failed: ${JSON.stringify(toHaruError(e))}`));
  }

  /** Debug query flags (`?debug&<name>`), same style as the renderer's. */
  private debugFlag(name: string): boolean {
    try {
      const q = new URLSearchParams(window.location.search);
      return q.has('debug') && q.has(name);
    } catch { return false; }
  }

  private async stop(): Promise<void> {
    if (this.phase !== 'talking') return;
    this.endTalk();
    try {
      await this.client.stopConversation();
    } catch (e) {
      this.toast.show(toHaruError(e));
    }
    if (!this.booting) this.showStart();
  }

  /** Local teardown; idempotent. */
  private endTalk(): void {
    if (this.phase !== 'talking' && this.phase !== 'starting') return;
    this.phase = 'idle';
    this.presenceActive = false;
    this.presence.end();
    if (this.forceTimer) { clearTimeout(this.forceTimer); this.forceTimer = null; }
    this.renderer.keepFaceReady(false);
    this.hud.hide();
    this.hud.setBattle(false);
    this.preparing.hide();
    this.renderer.resetWords();
    this.renderer.setMode('idle');
    this.renderer.setBattle(false);
    this.subtitles.clear();
    this.battle = false;
  }

  // ---------------------------------------------------------------- actions

  private async onHudAction(a: HudAction): Promise<void> {
    switch (a) {
      case 'mute': await this.toggleMute(); break;
      case 'settings': await this.toggleSettings(); break;
      case 'fullscreen': await this.toggleFullscreen(); break;
      case 'stop': await this.stop(); break;
    }
  }

  private async toggleMute(): Promise<void> {
    this.muted = !this.muted;
    this.hud.setMuted(this.muted);
    try {
      await this.client.setMuted(this.muted);
    } catch (e) {
      this.muted = !this.muted;
      this.hud.setMuted(this.muted);
      this.toast.show(toHaruError(e));
    }
  }

  private async toggleSettings(): Promise<void> {
    if (this.settingsPanel.isOpen) {
      this.settingsPanel.close();
      return;
    }
    try {
      const [s, a, b] = await Promise.all([this.client.getSettings(), this.client.listAudioDevices(), this.client.backendOptions()]);
      this.settingsPanel.open(s, a, b);
    } catch (e) {
      this.toast.show(toHaruError(e));
    }
  }

  private async toggleFullscreen(): Promise<void> {
    try {
      if ('__TAURI_INTERNALS__' in window) {
        const { getCurrentWindow } = await import('@tauri-apps/api/window');
        const w = getCurrentWindow();
        await w.setFullscreen(!(await w.isFullscreen()));
      } else if (document.fullscreenElement) {
        await document.exitFullscreen();
      } else {
        await document.documentElement.requestFullscreen();
      }
    } catch { /* fullscreen can be refused by the browser; nothing to report */ }
  }

  private async saveSettings(s: Settings): Promise<void> {
    const r = await this.client.saveSettings(s);
    this.applySettings(r.settings);
    if (r.reloadRequired) void this.runBoot();
  }

  private applySettings(s: Settings): void {
    this.settings = s;
    this.renderer.setQualityMode(s.quality);
    this.subtitles.setEnabled(s.subtitles);
    this.renderer.setFace(s.showFace ?? true, s.gender);
    if (resolveLanguage(s.language) !== getLanguage()) {
      setLanguage(s.language);
      this.relabel();
    }
  }

  /** Labels every overlay again after a language switch (subtitles show spoken words and stay). */
  private relabel(): void {
    this.boot.relabel();
    this.hint.relabel();
    this.chooser.relabel();
    this.topicSheet.relabel();
    this.preparing.relabel();
    this.hud.relabel();
    this.settingsPanel.relabel();
    this.toast.relabel();
  }
}
