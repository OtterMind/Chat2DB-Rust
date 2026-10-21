import { memo, useEffect, useRef } from 'react';
import * as monaco from 'monaco-editor';
import { useGlobalStore } from '@/store/global';
import { setupMonacoEnvironment } from '@/utils/monaco';
import { getContentDiffOpenBlockReason } from '@/components/SQLEditor/helper/contentDiffGuard';

interface ContentDiffTabProps {
  originalText?: string;
  modifiedText?: string;
  language?: string;
}

const ContentDiffTab = memo(({ originalText = '', modifiedText = '', language = 'sql' }: ContentDiffTabProps) => {
  const containerRef = useRef<HTMLDivElement>(null);
  const diffEditorRef = useRef<monaco.editor.IStandaloneDiffEditor | null>(null);
  const originalModelRef = useRef<monaco.editor.ITextModel | null>(null);
  const modifiedModelRef = useRef<monaco.editor.ITextModel | null>(null);
  const originalTextRef = useRef(originalText);
  const modifiedTextRef = useRef(modifiedText);
  const { appearance, globalEditorSettings, getEditorTheme } = useGlobalStore((s) => ({
    appearance: s.baseSetting.appearance,
    globalEditorSettings: s.editorSettings,
    getEditorTheme: s.getEditorTheme,
  }));

  useEffect(() => {
    if (!containerRef.current) {
      return;
    }
    if (getContentDiffOpenBlockReason(originalTextRef.current, modifiedTextRef.current)) {
      return;
    }

    setupMonacoEnvironment();

    const diffEditor = monaco.editor.createDiffEditor(containerRef.current, {
      ...globalEditorSettings,
      theme: getEditorTheme(appearance),
      automaticLayout: true,
      readOnly: true,
      originalEditable: false,
      renderSideBySide: true,
      renderMarginRevertIcon: false,
      minimap: { enabled: true },
      contextmenu: false,
      scrollBeyondLastLine: false,
    });
    const originalModel = monaco.editor.createModel(originalTextRef.current, language);
    const modifiedModel = monaco.editor.createModel(modifiedTextRef.current, language);

    diffEditor.setModel({
      original: originalModel,
      modified: modifiedModel,
    });

    diffEditorRef.current = diffEditor;
    originalModelRef.current = originalModel;
    modifiedModelRef.current = modifiedModel;

    return () => {
      diffEditor.dispose();
      originalModel.dispose();
      modifiedModel.dispose();
      diffEditorRef.current = null;
      originalModelRef.current = null;
      modifiedModelRef.current = null;
    };
  }, [appearance, getEditorTheme, globalEditorSettings, language]);

  useEffect(() => {
    originalTextRef.current = originalText;
    modifiedTextRef.current = modifiedText;
    if (getContentDiffOpenBlockReason(originalText, modifiedText)) {
      originalModelRef.current?.setValue('');
      modifiedModelRef.current?.setValue('');
      return;
    }

    if (originalModelRef.current) {
      monaco.editor.setModelLanguage(originalModelRef.current, language);
    }
    if (modifiedModelRef.current) {
      monaco.editor.setModelLanguage(modifiedModelRef.current, language);
    }
    originalModelRef.current?.setValue(originalText);
    modifiedModelRef.current?.setValue(modifiedText);
  }, [language, modifiedText, originalText]);

  return <div ref={containerRef} style={{ width: '100%', height: '100%' }} />;
});

export default ContentDiffTab;
