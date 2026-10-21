import { useEffect, useRef } from 'react';
import { isDesktop } from '@/utils/env';

const NON_TEXT_INPUT_TYPES = new Set([
  'button',
  'checkbox',
  'color',
  'file',
  'hidden',
  'image',
  'radio',
  'range',
  'reset',
  'submit',
]);

function resolveEditableElement(target: EventTarget | null): HTMLElement | null {
  if (!(target instanceof HTMLElement)) {
    return null;
  }

  const editable = target.closest('input, textarea, [contenteditable="true"], [contenteditable=""]');
  if (!(editable instanceof HTMLElement)) {
    return null;
  }

  if (editable instanceof HTMLInputElement) {
    if (editable.disabled || editable.readOnly || NON_TEXT_INPUT_TYPES.has(editable.type)) {
      return null;
    }
    return editable;
  }

  if (editable instanceof HTMLTextAreaElement) {
    if (editable.disabled || editable.readOnly) {
      return null;
    }
    return editable;
  }

  return editable.isContentEditable ? editable : null;
}

function focusEditableElement(element: HTMLElement) {
  window.setTimeout(() => {
    if (!document.contains(element)) {
      return;
    }

    element.focus({ preventScroll: true });

    if (element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement) {
      const start = element.selectionStart ?? element.value.length;
      const end = element.selectionEnd ?? start;
      try {
        element.setSelectionRange(start, end);
      } catch {
        // ignore
      }
    }
  }, 0);
}

const useDesktopInputFocusFix = () => {
  const lastEditableRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    if (!isDesktop) {
      return;
    }

    const handlePointerDownCapture = (event: PointerEvent) => {
      const editable = resolveEditableElement(event.target);
      if (!editable) {
        return;
      }
      lastEditableRef.current = editable;
      focusEditableElement(editable);
    };

    const handleFocusIn = (event: FocusEvent) => {
      const editable = resolveEditableElement(event.target);
      if (!editable) {
        return;
      }
      lastEditableRef.current = editable;
    };

    const handleWindowFocus = () => {
      const editable = lastEditableRef.current;
      if (!editable || !document.contains(editable)) {
        return;
      }
      focusEditableElement(editable);
    };

    document.addEventListener('pointerdown', handlePointerDownCapture, true);
    document.addEventListener('focusin', handleFocusIn, true);
    window.addEventListener('focus', handleWindowFocus);

    return () => {
      document.removeEventListener('pointerdown', handlePointerDownCapture, true);
      document.removeEventListener('focusin', handleFocusIn, true);
      window.removeEventListener('focus', handleWindowFocus);
    };
  }, []);
};

export default useDesktopInputFocusFix;
