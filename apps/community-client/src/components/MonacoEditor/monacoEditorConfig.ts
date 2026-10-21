import { IEditorOptions } from '@/components/MonacoEditor';

export const editorDefaultOptions: IEditorOptions = {
  fontFamily: 
   `"Menlo", 
    "DejaVu Sans Mono", 
    "Liberation Mono", 
    "Consolas", 
    "Ubuntu Mono", 
    "Courier New", 
    "andale mono", 
    "lucida console", 
    "monospace"`,
  scrollBeyondLastLine: false, // Scroll beyond the final line.
  automaticLayout: true, // Automatic layout.
  dragAndDrop: false, // Drag and drop.
  fontSize: 13, // Font size.
  tabSize: 2, // Tab size.
  lineHeight: 20, // Line height.
  theme: 'vscode', // Theme.
  roundedSelection: false, // Rounded selection.
  readOnly: false, // Read-only mode.
  // folding: false, // Hide folding controls.
  insertSpaces: true, // Insert spaces.
  autoClosingQuotes: 'always', // Close quotes automatically.
  detectIndentation: false, // Detect indentation.
  wordWrap: 'on', // Wrap lines automatically.
  fixedOverflowWidgets: true, // Keep overflow widgets fixed.
  unusualLineTerminators: 'off',
  // renderLineHighlight: 'none', // Render line highlighting.
  codeLens: false, // CodeLens.
  scrollbar: {
    // Scrollbar.
    alwaysConsumeMouseWheel: false, // Always consume mouse-wheel events.
  },
  unicodeHighlight: {
    ambiguousCharacters: false,
    invisibleCharacters: false,
  },
  minimap: {
    // Minimap.
    enabled: false, // Enabled.
  },
  // Line-number width.
  lineNumbersMinChars: 4,
  // Width to the right of line numbers.
  lineDecorationsWidth: 0,
};
