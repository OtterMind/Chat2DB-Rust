import { memo, useCallback, useMemo, ForwardedRef, forwardRef, useImperativeHandle, useRef } from 'react';
import styles from './index.less';
import classnames from 'classnames';
import MonacoEditor, { IExportRefFunction } from '@/components/MonacoEditor';
import { v4 as uuid } from 'uuid';

interface IProps {
  className?: string;
  handelEnter?: (value: string) => void;
  focusChange?: (isActive: boolean) => void;
  ref: any; // TODO: Move this ref to the appropriate owner.
}

export interface ISingleFileMonacoEditorRefFunction {
  getAllContent?: () => string;
  setValue?: (value: string) => void;
  onSearch?: () => void;
}

const options = {
  lineNumbers: false,
  renderLineHighlight: 'none',
  scrollBeyondLastLine: false,
  wordWrap: 'off',
  minimap: {
    enabled: false,
  },
  // Hide the scrollbar.
  scrollbar: {
    vertical: 'hidden',
    horizontal: 'hidden',
  },
  overviewRulerBorder: false,
  glyphMargin: false,
  folding: false,
  lineDecorationsWidth: 0, // Line-number width.
  lineNumbersMinChars: 0, // Minimum line-number width.
  lineHeight: 26,
};

const SingleFileMonacoEditor = memo<IProps>(
  forwardRef((props, ref: ForwardedRef<ISingleFileMonacoEditorRefFunction>) => {
    const { className, handelEnter, focusChange } = props;
    const editorRef = useRef<any>(null);
    const monacoEditorRef = useRef<IExportRefFunction>(null);

    const editorId = useMemo(() => {
      return uuid();
    }, []);

    const handleKeydown = useCallback((event) => {
      if (event.key === 'Enter' && editorRef.current) {
        const controller = editorRef.current.getContribution('editor.contrib.suggestController') as any;
        const suggestWidget = controller._widget;
        if (suggestWidget && suggestWidget.suggestWidgetVisible.get()) {
          return;
        }
        // Otherwise prevent Enter's default behavior.
        event.preventDefault();
        handleEnterSearch();
      }
    }, []);

    const handleEnterSearch = () => {
      const value = monacoEditorRef.current?.getAllContent().trim() || '';
      handelEnter && handelEnter(value);
    };

    // Listen for keydown and prevent Enter's default behavior.
    const registerShortcutKey = useCallback((_editor, _monaco, isActive) => {
      if (isActive) {
        editorRef.current = _editor;
        window.addEventListener('keydown', handleKeydown);
      } else {
        window.removeEventListener('keydown', handleKeydown);
      }
    }, []);

    const getAllContent = () => {
      return monacoEditorRef.current?.getAllContent() || '';
    };

    useImperativeHandle(ref, () => ({
      getAllContent,
      setValue: monacoEditorRef.current?.setValue,
      onSearch: handleEnterSearch,
    }));

    return (
      <div ref={ref as any} className={classnames(styles.singleFileMonacoEditor, className)}>
        <MonacoEditor
          ref={monacoEditorRef}
          id={editorId}
          disableFind
          options={options as any}
          shortcutKey={registerShortcutKey}
          focusChange={focusChange}
        />
      </div>
    );
  }),
);

export default SingleFileMonacoEditor;
