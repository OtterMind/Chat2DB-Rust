/**
 * setModelMarkers.ts
 * ---------------------
 * Set model markers.
 */
import * as monaco from 'monaco-editor';
import { MarkMessage } from '@/typings/sqlParser';

/**
 * Set model markers.
 *
 */
const setModelMarkers = (model: monaco.editor.ITextModel, id: string, markMessages: MarkMessage[]) => {
  const safeMarkMessages = Array.isArray(markMessages) ? markMessages : [];
  const markers: monaco.editor.IMarkerData[] = safeMarkMessages.map((item) => ({
    severity: item.type === 'warning' ? monaco.MarkerSeverity.Warning : monaco.MarkerSeverity.Error,
    message: item.message,
    startLineNumber: item.startLineNum,
    startColumn: item.startColNum,
    endLineNumber: item.endLineNum,
    endColumn: item.endColNum,
  }));

  monaco.editor.setModelMarkers(model, id, markers);
};

export { setModelMarkers };
