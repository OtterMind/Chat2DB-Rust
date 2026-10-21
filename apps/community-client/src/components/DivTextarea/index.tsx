import { memo, forwardRef, useImperativeHandle, ForwardedRef, useRef, useEffect } from 'react';
import { useStyles } from './style';

interface IProps {
  className?: string;
  onChange?: (value: string) => void;
  value: string;
}

export interface DivTextareaRef {
  focus: () => void;
  blur: () => void;
}

function isNodeInElement(element: HTMLElement, node: Node | null): boolean {
  if (!node) {
    return false;
  }
  return node === element || element.contains(node);
}

function setCaretToEnd(element: HTMLElement) {
  const range = document.createRange();
  range.selectNodeContents(element);
  range.collapse(false);
  const selection = window.getSelection();
  selection?.removeAllRanges();
  selection?.addRange(range);
}

function insertPlainText(element: HTMLElement, text: string) {
  const selection = window.getSelection();
  if (!selection || selection.rangeCount === 0 || !isNodeInElement(element, selection.anchorNode)) {
    element.focus();
    setCaretToEnd(element);
  }

  const activeSelection = window.getSelection();
  if (!activeSelection || activeSelection.rangeCount === 0) {
    element.textContent = `${element.textContent || ''}${text}`;
    setCaretToEnd(element);
    return;
  }

  const range = activeSelection.getRangeAt(0);
  range.deleteContents();
  const textNode = document.createTextNode(text);
  range.insertNode(textNode);
  range.setStartAfter(textNode);
  range.setEndAfter(textNode);
  activeSelection.removeAllRanges();
  activeSelection.addRange(range);
}

const DivTextarea = forwardRef((props: IProps, ref: ForwardedRef<DivTextareaRef>) => {
  const { className, onChange, value } = props;
  const { styles, cx } = useStyles();
  const divTextareaRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    divTextareaRef.current!.textContent = value;

    const listenerInput = (event) => {
      onChange && onChange(event.target.outerText);
    };
    const listenerPaste = (event: ClipboardEvent) => {
      const text = event.clipboardData?.getData('text/plain');
      if (text === undefined) {
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      insertPlainText(divTextareaRef.current!, text);
      onChange && onChange(divTextareaRef.current?.textContent || '');
    };
    const insertLineBreak = () => {
      const selection = window.getSelection();
      if (selection?.rangeCount) {
        const range = selection.getRangeAt(0);
        const br = document.createElement('br');
        range.deleteContents(); // Delete the current selection.
        range.insertNode(br); // Insert <br>.
        range.setStartAfter(br); // Move the caret after <br>.
        range.setEndAfter(br);
        selection.removeAllRanges();
        selection.addRange(range);
      }
    };

    const listenerKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Enter') {
        if (event.shiftKey) {
          event.stopPropagation();
          insertLineBreak();
          const text = divTextareaRef.current?.innerHTML.replace(/<br>/g, '\n'); // Replace <br> with \n.
          onChange && onChange(text || '');
        }
      }
    };

    divTextareaRef.current?.addEventListener('keydown', listenerKeyDown);
    divTextareaRef.current?.addEventListener('paste', listenerPaste);
    divTextareaRef.current?.addEventListener('input', listenerInput);

    return () => {
      divTextareaRef.current?.removeEventListener('keydown', listenerKeyDown);
      divTextareaRef.current?.removeEventListener('paste', listenerPaste);
      divTextareaRef.current?.removeEventListener('input', listenerInput);
    };
  }, []);

  const handleFocus = () => {
    // divTextareaRef.current?.focus();
    const el = divTextareaRef.current;
    if (el) {
      el.focus();
      const range = document.createRange();
      range.selectNodeContents(el);
      range.collapse(false); // Move the caret to the end of the content.
      const selection = window.getSelection();
      selection?.removeAllRanges();
      selection?.addRange(range);
    }
  };

  const handleBlur = () => {
    divTextareaRef.current?.blur();
  };

  const handleSelectAll = () => {
    const el = divTextareaRef.current;
    if (el) {
      const range = document.createRange();
      range.selectNodeContents(el);
      const selection = window.getSelection();
      selection?.removeAllRanges();
      selection?.addRange(range);
    }
  };

  useImperativeHandle(ref, () => ({
    focus: handleFocus,
    blur: handleBlur,
    selectAll: handleSelectAll,
    getValue: () => divTextareaRef.current?.textContent,
    textarea: divTextareaRef.current,
  }));

  return <div contentEditable spellCheck={false} ref={divTextareaRef} className={cx(className, styles.container)} />;
});

export default memo(DivTextarea);
