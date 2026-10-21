import { memo, useEffect, useRef, useState, forwardRef, ForwardedRef, useImperativeHandle } from 'react';
import { useStyles } from './style';

interface IProps {
  className?: string;
  fieldPrompt: string;
  value?: string;
  minWidth?: number;
  autoFocus?: boolean;
  onEnter?: (res?: any) => void;
  maxLines?: number;
}

export interface FieldPromptInputRef {
  getValue: () => string;
}

const FieldPromptInput = forwardRef((props: IProps, ref: ForwardedRef<FieldPromptInputRef>) => {
  const { className, fieldPrompt, value = '', minWidth = 45, maxLines, onEnter } = props;
  const { styles, cx } = useStyles({ minWidth: minWidth, maxLines });
  const contentEditableSpanRef = useRef<HTMLSpanElement>(null);
  const [isActive, setIsActive] = useState(false);
  const [isInputting, setIsInputting] = useState(false);

  const handleKeyDown = (event) => {
    if (event.key === 'Enter') {
      // Prevent Enter from inserting a newline.
      event.preventDefault();
      // Stop event propagation.
      event.stopPropagation();
      // Invoke onEnter only when the user is not composing text.
      if (!isInputting) {
        onEnter?.();
      }
      return;
    }
  };

  useEffect(() => {
    const span = contentEditableSpanRef.current;
    if (span) {
      span.innerText = value;
    }
  }, [value]);

  useEffect(() => {
    const span = contentEditableSpanRef.current;
    if (span) {
      // Add the event listener.
      const handleFocus = () => setIsActive(true);
      const handleBlur = () => setIsActive(false);

      span.addEventListener('focus', handleFocus);
      span.addEventListener('blur', handleBlur);

      // Remove the event listener.
      return () => {
        span.removeEventListener('focus', handleFocus);
        span.removeEventListener('blur', handleBlur);
      };
    }
  }, []);

  useEffect(() => {
    if (props.autoFocus && contentEditableSpanRef.current) {
      contentEditableSpanRef.current.focus();
    }
  }, []);

  const getValue = () => {
    const span = contentEditableSpanRef.current;
    return span ? span.innerText : '';
  };

  useImperativeHandle(
    ref,
    () => ({
      getValue,
    }),
    [],
  );

  return (
    <span className={cx(styles.fieldPromptInput, { [styles.fieldPromptInputActive]: isActive }, className)}>
      <span className="fieldPrompt">{fieldPrompt}</span>
      <span
        ref={contentEditableSpanRef}
        onFocus={(event) => {
          event.stopPropagation();
        }}
        onPaste={(event) => {
          event.preventDefault();
          const text = event.clipboardData.getData('text/plain');
          const selection = document.getSelection();
          if (!selection) {
            return;
          }
          selection.deleteFromDocument();
          const range = selection.getRangeAt(0);
          const textNode = document.createTextNode(text);
          range.insertNode(textNode);

          // Clear the selection or move the caret to the end of the text.
          range.setStartAfter(textNode);
          range.setEndAfter(textNode);
          selection.removeAllRanges();
          selection.addRange(range);
        }}
        className="contentEditableSpan"
        contentEditable
        onKeyDown={handleKeyDown}
        onCompositionStart={() => setIsInputting(true)}
        onCompositionEnd={() => setIsInputting(false)}
      />
    </span>
  );
});

export default memo(FieldPromptInput);
