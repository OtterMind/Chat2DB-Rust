import * as monaco from 'monaco-editor';
import { ParameterHintContext } from '../helper/parameterHint';

const PARAMETER_HINT_WIDGET_ID = 'editor.widget.parameterHint';
const PARAMETER_HINT_WIDGET_KEY = '__chat2dbParameterHintWidget';
const PARAMETER_HINT_WIDGET_CLASS = 'parameter-hint-widget';
const PARAMETER_HINT_ITEM_CLASS = 'parameter-hint-item';
const PARAMETER_HINT_ITEM_ACTIVE_CLASS = 'active';
const PARAMETER_HINT_SEPARATOR_CLASS = 'parameter-hint-separator';

class ParameterHintWidget implements monaco.editor.IContentWidget {
  private domNode: HTMLElement | undefined;
  private context: ParameterHintContext | null = null;
  private mounted = false;
  private disposed = false;

  constructor(private readonly editor: monaco.editor.IStandaloneCodeEditor) {
    const existingWidget = (editor as any)[PARAMETER_HINT_WIDGET_KEY] as ParameterHintWidget | undefined;
    if (existingWidget && existingWidget !== this) {
      existingWidget.dispose();
    }
    (editor as any)[PARAMETER_HINT_WIDGET_KEY] = this;
  }

  getId(): string {
    return PARAMETER_HINT_WIDGET_ID;
  }

  getDomNode(): HTMLElement {
    if (this.disposed) {
      return document.createElement('div');
    }
    if (!this.domNode) {
      this.domNode = document.createElement('div');
      this.domNode.className = PARAMETER_HINT_WIDGET_CLASS;
      this.domNode.style.width = 'max-content';
      this.domNode.style.pointerEvents = 'auto';
      this.editor.applyFontInfo(this.domNode);
    }
    this.render();
    return this.domNode;
  }

  getPosition(): monaco.editor.IContentWidgetPosition | null {
    if (!this.context) {
      return null;
    }

    return {
      position: {
        lineNumber: this.context.anchorRange.startLineNumber,
        column: this.context.anchorRange.startColumn,
      },
      preference: [
        monaco.editor.ContentWidgetPositionPreference.ABOVE,
        monaco.editor.ContentWidgetPositionPreference.BELOW,
      ],
    };
  }

  show(context: ParameterHintContext): void {
    if (this.disposed) {
      return;
    }
    this.context = context;
    if (!this.mounted) {
      this.editor.addContentWidget(this);
      this.mounted = true;
    }
    this.render();
    this.editor.layoutContentWidget(this);
  }

  hide(): void {
    this.context = null;
    if (this.mounted) {
      try {
        this.editor.removeContentWidget(this);
      } catch {
        // Monaco may already be disposed during React unmount.
      }
      this.mounted = false;
    }
  }

  dispose(): void {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    if ((this.editor as any)[PARAMETER_HINT_WIDGET_KEY] === this) {
      delete (this.editor as any)[PARAMETER_HINT_WIDGET_KEY];
    }
    this.hide();
    this.domNode?.remove();
    this.domNode = undefined;
  }

  private render(): void {
    if (!this.domNode) {
      return;
    }

    if (!this.context) {
      this.domNode.textContent = '';
      return;
    }

    this.domNode.textContent = '';
    let activeNode: HTMLElement | null = null;
    this.context.items.forEach((hint, index) => {
      if (index > 0) {
        const separator = document.createElement('span');
        separator.className = PARAMETER_HINT_SEPARATOR_CLASS;
        separator.textContent = ', ';
        this.domNode!.appendChild(separator);
      }

      const item = document.createElement('span');
      item.className = hint.active
        ? `${PARAMETER_HINT_ITEM_CLASS} ${PARAMETER_HINT_ITEM_ACTIVE_CLASS}`
        : PARAMETER_HINT_ITEM_CLASS;
      item.textContent = hint.label;
      item.title = hint.label;
      this.domNode!.appendChild(item);
      if (hint.active) {
        activeNode = item;
      }
    });
    this.scrollHintIntoView(activeNode);
  }

  private scrollHintIntoView(activeNode: HTMLElement | null): void {
    if (!this.domNode || !activeNode) {
      return;
    }

    const visibleLeft = this.domNode.scrollLeft;
    const visibleRight = visibleLeft + this.domNode.clientWidth;
    const itemLeft = activeNode.offsetLeft;
    const itemRight = itemLeft + activeNode.offsetWidth;

    if (itemLeft < visibleLeft) {
      this.domNode.scrollLeft = Math.max(0, itemLeft - 6);
    } else if (itemRight > visibleRight) {
      this.domNode.scrollLeft = itemRight - this.domNode.clientWidth + 6;
    }
  }
}

export default ParameterHintWidget;
