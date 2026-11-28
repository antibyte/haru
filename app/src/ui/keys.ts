export interface KeyHandlers { mute(): void; settings(): void; fullscreen(): void; escape(): void }

const EDITABLE = new Set(['INPUT', 'TEXTAREA', 'SELECT']);

/**
 * Global shortcuts: M mute, S settings, F fullscreen, Escape. Typing in form fields is never hijacked
 * (Escape still passes through so the settings drawer can always be dismissed). Returns an unbind function.
 */
export function bindKeys(h: KeyHandlers): () => void {
  const onKey = (e: KeyboardEvent) => {
    if (e.ctrlKey || e.altKey || e.metaKey || e.repeat) return;
    if (e.isComposing) return;
    const t = e.target as HTMLElement | null;
    const editable = !!t && (EDITABLE.has(t.tagName) || t.isContentEditable);
    if (e.key === 'Escape') { h.escape(); return; }
    if (editable) return;
    switch (e.key.toLowerCase()) {
      case 'm': h.mute(); break;
      case 's': h.settings(); break;
      case 'f': h.fullscreen(); break;
      default: return;
    }
    e.preventDefault();
  };
  window.addEventListener('keydown', onKey);
  return () => window.removeEventListener('keydown', onKey);
}
