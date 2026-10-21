import { useEffect } from 'react';

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

function isEditableElement(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) {
    return false;
  }

  const editable = target.closest('input, textarea, [contenteditable="true"], [contenteditable=""]');
  if (!(editable instanceof HTMLElement)) {
    return false;
  }

  if (editable instanceof HTMLInputElement) {
    return !editable.disabled && !editable.readOnly && !NON_TEXT_INPUT_TYPES.has(editable.type);
  }

  if (editable instanceof HTMLTextAreaElement) {
    return !editable.disabled && !editable.readOnly;
  }

  return editable.isContentEditable;
}

function useDocumentListener() {
  useEffect(() => {
    //Block the global default cmd+f
    const handleKeyDown = (e: KeyboardEvent) => {
      if (isEditableElement(e.target)) {
        return;
      }
      if (e.code === 'KeyF' && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
      }
    };
    document.addEventListener('keydown', handleKeyDown);
    return () => {
      document.removeEventListener('keydown', handleKeyDown);
    };
  }, []);
}

export default useDocumentListener;
