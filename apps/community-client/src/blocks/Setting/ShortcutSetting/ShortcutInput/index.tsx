import React, { useEffect, useRef, useState } from 'react';
import { useStyles } from './style';
import { normalizeShortcutBinding } from '@/constants/shortcut';
import { i18n } from '@/i18n';

interface IProps {
  value: string | null;
  onChange: (value: string) => void;
  disabled?: boolean;
  placeholder?: string;
}

const keySymbolMap: Record<string, string> = {
  meta: '⌘',
  control: 'Ctrl',
  alt: 'Alt',
  shift: 'Shift',
};
// Sort modifier keys in the specified order
const modifierOrder = {
  meta: 1,
  control: 1, // meta and control are the same level
  alt: 2,
  shift: 3,
};

const modifierKeys = new Set(['meta', 'control', 'alt', 'shift']);
const singleKeyShortcuts = new Set([
  'enter',
  'escape',
  'tab',
  'backspace',
  'delete',
  'f1',
  'f2',
  'f3',
  'f4',
  'f5',
  'f6',
  'f7',
  'f8',
  'f9',
  'f10',
  'f11',
  'f12',
]);

const getShortcutKeys = (shortcut?: string | null) => {
  const normalizedShortcut = normalizeShortcutBinding(shortcut);
  if (!normalizedShortcut) {
    return [];
  }
  return normalizedShortcut.split(' + ');
};

const ShortcutInput: React.FC<IProps> = ({ value, onChange, disabled, placeholder }) => {
  const { styles, cx } = useStyles();
  const [displayValue, setDisplayValue] = useState(normalizeShortcutBinding(value) || '');
  const [focused, setFocused] = useState(false);
  const keysPressed = useRef<Set<string>>(new Set());
  const pendingShortcutRef = useRef(value || '');

  useEffect(() => {
    setDisplayValue(normalizeShortcutBinding(value) || '');
    pendingShortcutRef.current = value || '';
  }, [value]);

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (disabled) return;
    e.preventDefault();

    const key = e.key.toLowerCase();

    keysPressed.current.add(key);

    const shortcut = Array.from(keysPressed.current)
      .sort((a, b) => {
        const orderA = modifierOrder[a as keyof typeof modifierOrder] || 999;
        const orderB = modifierOrder[b as keyof typeof modifierOrder] || 999;
        return orderA - orderB;
      })
      .map((k) => keySymbolMap[k] || k.charAt(0).toUpperCase() + k.slice(1))
      .join(' + ');

    const normalizedShortcut = normalizeShortcutBinding(shortcut) || shortcut;
    pendingShortcutRef.current = normalizedShortcut;
    setDisplayValue(normalizedShortcut);
  };

  const handleKeyUp = () => {
    if (disabled) return;
    if (!keysPressed.current.size) return;

    // Restore the original value for incomplete shortcuts without a supported single key or modifier.
    const pressedKeys = Array.from(keysPressed.current);
    const hasModifierKey = pressedKeys.some((key) => modifierKeys.has(key));
    const isAllowedSingleKey = pressedKeys.length === 1 && singleKeyShortcuts.has(pressedKeys[0]);

    if (!isAllowedSingleKey && (keysPressed.current.size <= 1 || !hasModifierKey)) {
      keysPressed.current.clear();
      setDisplayValue(normalizeShortcutBinding(value) || '');
      return;
    }

    keysPressed.current.clear();
    // only updates shortcut keys when out of focus
    const normalizedValue = normalizeShortcutBinding(pendingShortcutRef.current);
    if (normalizedValue && normalizedValue !== value) {
      onChange(normalizedValue);
    }
  };

  const shortcutKeys = getShortcutKeys(displayValue);

  return (
    <button
      className={cx(styles.shortcutInput, focused && styles.shortcutInputFocused)}
      type="button"
      disabled={disabled}
      onKeyDown={handleKeyDown}
      onKeyUp={handleKeyUp}
      onFocus={() => setFocused(true)}
      onBlur={() => setFocused(false)}
    >
      {shortcutKeys.length ? (
        shortcutKeys.map((key) => (
          <span className={styles.shortcutKey} key={key}>
            {key}
          </span>
        ))
      ) : (
        <span className={styles.placeholder}>{placeholder || i18n('setting.shortcut.placeholder.input')}</span>
      )}
    </button>
  );
};

export default ShortcutInput;
