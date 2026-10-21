import * as React from 'react';
import * as ReactDOM from 'react-dom/client';
import * as monaco from 'monaco-editor/esm/vs/editor/editor.api';
import AppTheme from '@/components/AppTheme';

const PLACEHOLDER_WIDGET_KEY = '__chat2dbPlaceholderWidget';

class PlaceholderContentWidget implements monaco.editor.IContentWidget {
  private static readonly ID = 'editor.widget.placeholderHint';

  private domNode: HTMLElement | undefined;
  private readonly disposable: monaco.IDisposable;
  private root: ReactDOM.Root | undefined;
  private mounted = false;

  constructor(private readonly placeholder: React.ReactNode, private readonly editor: monaco.editor.ICodeEditor) {
    const existingWidget = (editor as any)[PLACEHOLDER_WIDGET_KEY] as PlaceholderContentWidget | undefined;
    if (existingWidget && existingWidget !== this) {
      existingWidget.allDispose();
    }
    (editor as any)[PLACEHOLDER_WIDGET_KEY] = this;
    // register a listener for editor code changes
    this.disposable = editor.onDidChangeModelContent(() => this.onDidChangeModelContent());
    // ensure that on initial load the placeholder is shown
    this.onDidChangeModelContent();
  }

  private onDidChangeModelContent(): void {
    if (this.editor.getValue() === '') {
      if (!this.mounted) {
        this.editor.addContentWidget(this);
        this.mounted = true;
      }
    } else {
      this.dispose();
    }
  }

  getId(): string {
    return PlaceholderContentWidget.ID;
  }

  getDomNode(): HTMLElement {
    if (!this.domNode) {
      this.domNode = document.createElement('div');
      this.domNode.style.width = 'max-content';
      this.domNode.style.pointerEvents = 'none';
      // this.domNode.style.fontStyle = 'italic';
      this.editor.applyFontInfo(this.domNode);
      this.root = ReactDOM.createRoot(this.domNode);
      this.root.render(<AppTheme>{this.placeholder}</AppTheme>);
    }

    return this.domNode;
  }

  getPosition(): monaco.editor.IContentWidgetPosition | null {
    return {
      position: { lineNumber: 1, column: 1 },
      preference: [monaco.editor.ContentWidgetPositionPreference.EXACT],
    };
  }

  dispose(): void {
    if ((this.editor as any)[PLACEHOLDER_WIDGET_KEY] === this) {
      delete (this.editor as any)[PLACEHOLDER_WIDGET_KEY];
    }
    if (this.mounted) {
      this.editor.removeContentWidget(this);
      this.mounted = false;
    }
  }

  allDispose(): void {
    this.disposable.dispose();
    this.root?.unmount();
    this.dispose();
  }
}

export default PlaceholderContentWidget;
