import * as monaco from 'monaco-editor';
import { EditorSetValueType } from '../type';

export const handleSetValue = (
  editor: monaco.editor.IStandaloneCodeEditor,
  value: string,
  type: EditorSetValueType,
) => {
  if (!editor) return;

  const model = editor.getModel();
  if (!model) return;

  switch (type) {
    case 'cover':
      coverEditor(editor, model, value);
      break;
    case 'end':
      insertAtEnd(editor, model, value);
      break;
    case 'start':
      insertAtStart(editor, value);
      break;
    case 'replace':
      replaceSelection(editor, value);
      break;
    case 'cursor':
      insertAtCursor(editor, value);
      break;
    case 'reset':
      resetEditor(editor, value);
      break;
    default:
      break;
  }
};

const insertAtEnd = (editor: monaco.editor.IStandaloneCodeEditor, model: monaco.editor.ITextModel, value: string) => {
  const lastLineNumber = model.getLineCount();
  const lastLineColumn = model.getLineMaxColumn(lastLineNumber);
  const range = new monaco.Range(lastLineNumber, lastLineColumn, lastLineNumber, lastLineColumn);
  editor.executeEdits('', [{ range, text: value, forceMoveMarkers: true }]);
  editor.revealLine(lastLineNumber + 1);
};

const insertAtStart = (editor: monaco.editor.IStandaloneCodeEditor, value: string) => {
  editor.executeEdits('', [{ range: new monaco.Range(1, 1, 1, 1), text: value, forceMoveMarkers: true }]);
};

const replaceSelection = (editor: monaco.editor.IStandaloneCodeEditor, value: string) => {
  const selection = editor.getSelection();
  if (selection) {
    editor.executeEdits('', [{ range: selection, text: value, forceMoveMarkers: true }]);
  }
};

const insertAtCursor = (editor: monaco.editor.IStandaloneCodeEditor, value: string) => {
  const position = editor.getPosition();
  if (position) {
    const range = new monaco.Range(position.lineNumber, position.column, position.lineNumber, position.column);
    editor.executeEdits('', [{ range, text: value, forceMoveMarkers: true }]);
    editor.setPosition({ lineNumber: position.lineNumber, column: position.column + value.length });
  }
};

const resetEditor = (editor: monaco.editor.IStandaloneCodeEditor, value?: string) => {
  editor.setValue(value || '');
};

    // Allow undo after covering the value.
const coverEditor = (editor: monaco.editor.IStandaloneCodeEditor, model: monaco.editor.ITextModel, value: string) => { 
  const range = model.getFullModelRange();
  editor.executeEdits('', [{ range, text: value, forceMoveMarkers: true }]);
}
