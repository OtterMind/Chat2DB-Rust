const RESULT_SET_EDITOR_MAP: Record<string, string> = {
  DATE: 'custom-date-editor',
  TIME: 'custom-time-editor',
  DATETIME: 'custom-datetime-editor',
  TIMESTAMP: 'custom-timestamp-editor',
};

export const resolveResultSetEditor = (editorType?: string) => {
  return editorType ? RESULT_SET_EDITOR_MAP[editorType] || 'custom-input-editor' : 'custom-input-editor';
};
