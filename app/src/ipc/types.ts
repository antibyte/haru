export const STATE_EVENT = 'haru:state';
export type AppStateName = 'booting' | 'downloading' | 'loading' | 'ready' | 'talking' | 'error';
export type Backend = 'cuda' | 'vulkan' | 'metal' | 'cpu';
export type ErrorCode = 'download' | 'hashMismatch' | 'diskSpace' | 'noBackend' | 'worker' | 'noMicrophone'
  | 'audioDevice' | 'lagLimit' | 'settings' | 'io' | 'busy' | 'internal';
export interface HaruError { code: ErrorCode; message: string; detail: string | null }
export interface StateEvent { state: AppStateName; backend: Backend | null; device: string | null; error: HaruError | null; warning: string | null; notice?: string | null }
export type DownloadEvent =
  | { event: 'progress'; data: { downloaded: number; total: number; bytesPerSec: number } }
  | { event: 'verifying'; data: { done: number; total: number } }
  | { event: 'done'; data: { path: string } }
  | { event: 'error'; data: { message: string } };
export interface WordPiece { seq: number; text: string }
export interface VizFrame { t: number; model: { rms: number; bands: number[] }; mic: { rms: number; bands: number[] }; latencyMs: number; words: WordPiece[] }
export type Quality = 'auto' | 'low' | 'medium' | 'high' | 'ultra';
export type QualityLevel = Exclude<Quality, 'auto'>;
export const LANGUAGES = ['de', 'en', 'es', 'fr', 'ja', 'ko', 'pt', 'ru', 'zh'] as const;
export interface Settings {
  language: string; name: string; persona: string; gender: 'female' | 'male' | null; backchannel: 'low' | 'mid' | 'high' | null;
  speakFirst: boolean; temperature: number; topK: number; voicePath: string | null; inputDevice: string | null; outputDevice: string | null;
  backend: 'auto' | Backend; device: string; quality: Quality; subtitles: boolean; showFace: boolean; lagLimitSecs: number; headphoneHintSeen: boolean;
  echoCancellation: boolean;
}
export const DEFAULT_SETTINGS: Settings = { language: 'de', name: 'Haru', persona: '', gender: null, backchannel: null, speakFirst: true,
  temperature: 0.7, topK: 25, voicePath: null, inputDevice: null, outputDevice: null, backend: 'auto', device: 'auto', quality: 'auto',
  subtitles: false, showFace: true, lagLimitSecs: 1.0, headphoneHintSeen: false,
  echoCancellation: true };
export interface SaveSettingsResult { settings: Settings; reloadRequired: boolean }
export interface AudioDevices { inputs: string[]; outputs: string[]; defaultInput: string | null; defaultOutput: string | null }
export interface DeviceInfo { variant: Backend; name: string; description: string; kind: 'cpu' | 'gpu' | 'igpu'; memoryTotal: number }
export interface BackendOptions { variants: Backend[]; devices: DeviceInfo[]; cudaAvailable: boolean; backendsDir: string | null }
export interface AttemptInfo { backend: Backend; device: string; description: string }
export interface BootInfo { backend: Backend; device: string; deviceName: string; modelPath: string; plan: AttemptInfo[]; warning: string | null }

export interface HaruClient {
  getStatus(): Promise<StateEvent>;
  onState(cb: (e: StateEvent) => void): Promise<() => void>;
  getSettings(): Promise<Settings>;
  saveSettings(s: Settings): Promise<SaveSettingsResult>;
  listAudioDevices(): Promise<AudioDevices>;
  backendOptions(): Promise<BackendOptions>;
  boot(onDownload: (e: DownloadEvent) => void): Promise<BootInfo>;
  startConversation(onViz: (f: VizFrame) => void): Promise<void>;
  stopConversation(): Promise<void>;
  setMuted(muted: boolean): Promise<void>;
}
