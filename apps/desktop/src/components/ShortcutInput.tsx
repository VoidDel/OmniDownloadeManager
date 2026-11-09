// Records a global shortcut: focus the field and press the keys. The value is an accelerator as
// the backend registers it (`CommandOrControl+Alt+D`); Backspace or the ✕ clears it, Esc keeps
// what was there.

import type { Platform } from 'omni-ui';
import { useState, type KeyboardEvent } from 'react';
import { useTranslation } from 'react-i18next';

import { Icon } from './Icon';

const MODIFIER_CODES = /^(Control|Shift|Alt|Meta|OS)(Left|Right)?$/;

/** The accelerator for a key press, or null while only modifiers are down (or for a plain key
 *  without one: only F1–F24 may stand alone). */
export function accelerator(e: Pick<KeyboardEvent, 'code' | 'ctrlKey' | 'metaKey' | 'altKey' | 'shiftKey'>, mac: boolean): string | null {
  if (!e.code || MODIFIER_CODES.test(e.code)) return null;
  const mods: string[] = [];
  if (mac ? e.metaKey : e.ctrlKey) mods.push('CommandOrControl');
  if (mac && e.ctrlKey) mods.push('Control');
  if (!mac && e.metaKey) mods.push('Super');
  if (e.altKey) mods.push('Alt');
  if (e.shiftKey) mods.push('Shift');
  const fkey = /^F([1-9]|1\d|2[0-4])$/.test(e.code);
  if (!mods.length && !fkey) return null;
  return [...mods, e.code].join('+');
}

/** How an accelerator reads: `Ctrl+Alt+D` (Windows, Linux), `⌘⌥D` (macOS). */
export function shortcutLabel(acc: string, mac: boolean): string {
  const names: Record<string, string> = mac
    ? { CommandOrControl: '⌘', Control: '⌃', Alt: '⌥', Shift: '⇧', Super: '⌘' }
    : { CommandOrControl: 'Ctrl', Control: 'Ctrl', Alt: 'Alt', Shift: 'Shift', Super: 'Win' };
  const parts = acc.split('+').map((p) => names[p] ?? p.replace(/^Key|^Digit/, '').replace(/^Arrow/, ''));
  return parts.join(mac ? '' : '+');
}

export function ShortcutInput({ value, onChange, label, platform }: { value: string; onChange: (v: string) => void; label: string; platform: Platform }) {
  const { t } = useTranslation();
  const [recording, setRecording] = useState(false);
  const mac = platform === 'mac';
  const onKeyDown = (e: KeyboardEvent<HTMLButtonElement>) => {
    if (e.key === 'Tab') return;
    // The window's own shortcuts (Ctrl+N…) must not run while recording.
    e.preventDefault();
    e.stopPropagation();
    if (e.key === 'Escape' && !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey) return e.currentTarget.blur();
    if ((e.key === 'Backspace' || e.key === 'Delete') && !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey) {
      e.currentTarget.blur();
      return onChange('');
    }
    const acc = accelerator(e, mac);
    if (acc) {
      e.currentTarget.blur();
      if (acc !== value) onChange(acc);
    }
  };
  return (
    <span className="shortcut">
      <button
        type="button"
        className={`field-look shortcut-key${recording ? ' rec' : ''}`}
        aria-label={label}
        onFocus={() => setRecording(true)}
        onBlur={() => setRecording(false)}
        onKeyDown={onKeyDown}
      >
        {recording ? t('settings.general.shortcutRecording') : value ? shortcutLabel(value, mac) : t('settings.general.shortcutNone')}
      </button>
      {/* Hidden rather than gone, so the fields of both shortcuts line up. */}
      <button
        type="button"
        className="iconbtn"
        aria-label={t('settings.general.shortcutClear')}
        title={t('settings.general.shortcutClear')}
        style={value && !recording ? undefined : { visibility: 'hidden' }}
        onClick={() => onChange('')}
      >
        <Icon name="x" />
      </button>
    </span>
  );
}
