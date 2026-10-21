import type * as monaco from 'monaco-editor';

export const BACKEND_COMPLETION_SUGGEST_BODY_CLASS = 'chat2db-backend-completion';

const BOUNDED_SUGGEST_WIDGET_KEY = '__chat2dbBoundedSuggestWidget';
const ORIGINAL_ALLOW_EDITOR_OVERFLOW_KEY = '__chat2dbOriginalAllowEditorOverflow';
const DEFAULT_MIN_WIDTH = 360;
const DEFAULT_FALLBACK_WIDTH = 420;
const DEFAULT_MAX_WIDTH = 1280;
const DEFAULT_VIEWPORT_MARGIN = 48;
const DEFAULT_EXTRA_WIDTH = 160;
const DEFAULT_PART_GAP = 20;
const DEFAULT_EDITOR_BOUNDARY_MARGIN = 8;
const DEFAULT_EDITOR_MAX_WIDTH_RATIO = 0.72;
const DEFAULT_EDITOR_ABSOLUTE_MAX_WIDTH = 960;
const DEFAULT_MIN_BELOW_HEIGHT = 160;
const CONTENT_WIDGET_POSITION_BELOW = 2;

export interface CompletionSuggestWidthOptions {
  minWidth?: number;
  maxWidth?: number;
  fallbackWidth?: number;
  extraWidth?: number;
  partGap?: number;
  measureText?: (text: string) => number;
}

export interface CompletionLabelParts {
  label: string;
  detail: string;
  description: string;
}

let measureContext: CanvasRenderingContext2D | null | undefined;
const boundedSuggestWidgets = new Map<any, {
  editor: monaco.editor.ICodeEditor;
  originalAllowEditorOverflow: boolean;
}>();

export function getCompletionLabelParts(
  suggestion: Pick<monaco.languages.CompletionItem, 'label'>,
): CompletionLabelParts {
  const { label } = suggestion;
  if (typeof label === 'string') {
    return {
      label,
      detail: '',
      description: '',
    };
  }
  return {
    label: label?.label || '',
    detail: label?.detail || '',
    description: label?.description || '',
  };
}

export function measureCompletionSuggestWidth(
  suggestions: Pick<monaco.languages.CompletionItem, 'label'>[],
  options: CompletionSuggestWidthOptions = {},
): number {
  const minWidth = options.minWidth ?? DEFAULT_MIN_WIDTH;
  const maxWidth = options.maxWidth ?? getViewportMaxWidth(minWidth);
  const fallbackWidth = options.fallbackWidth ?? DEFAULT_FALLBACK_WIDTH;
  const extraWidth = options.extraWidth ?? DEFAULT_EXTRA_WIDTH;
  const partGap = options.partGap ?? DEFAULT_PART_GAP;
  const measureText = options.measureText ?? getDefaultMeasureText();

  if (!suggestions.length) {
    return clamp(fallbackWidth, minWidth, maxWidth);
  }

  const maxContentWidth = suggestions.reduce((currentMaxWidth, suggestion) => {
    const parts = getCompletionLabelParts(suggestion);
    const visibleParts = [parts.label, parts.detail, parts.description].filter(Boolean);
    const contentWidth = visibleParts.reduce((sum, part) => sum + measureText(part), 0)
      + Math.max(0, visibleParts.length - 1) * partGap;
    return Math.max(currentMaxWidth, contentWidth);
  }, 0);

  return Math.ceil(clamp(maxContentWidth + extraWidth, minWidth, maxWidth));
}

export function applyBackendCompletionSuggestWidth(
  suggestions: Pick<monaco.languages.CompletionItem, 'label'>[],
  editor?: monaco.editor.ICodeEditor | null,
): void {
  const width = measureCompletionSuggestWidth(suggestions, {
    maxWidth: getEditorSuggestMaxWidth(editor),
  });
  if (typeof document === 'undefined') {
    return;
  }
  document.body.classList.add(BACKEND_COMPLETION_SUGGEST_BODY_CLASS);
  prepareSuggestWidgetForEditorBoundary(editor);
  scheduleSuggestWidgetLayout(editor, width);
}

export function clearBackendCompletionSuggestWidth(): void {
  if (typeof document === 'undefined') {
    return;
  }
  document.body.classList.remove(BACKEND_COMPLETION_SUGGEST_BODY_CLASS);
  boundedSuggestWidgets.forEach(({ editor, originalAllowEditorOverflow }, contentWidget) => {
    contentWidget.allowEditorOverflow = originalAllowEditorOverflow;
    contentWidget[BOUNDED_SUGGEST_WIDGET_KEY] = false;
    if (contentWidget._added) {
      editor.removeContentWidget(contentWidget);
      editor.addContentWidget(contentWidget);
      editor.layoutContentWidget(contentWidget);
    }
  });
  boundedSuggestWidgets.clear();
}

function getViewportMaxWidth(minWidth: number): number {
  if (typeof window === 'undefined' || !Number.isFinite(window.innerWidth)) {
    return DEFAULT_MAX_WIDTH;
  }
  return Math.max(minWidth, window.innerWidth - DEFAULT_VIEWPORT_MARGIN);
}

function getDefaultMeasureText(): (text: string) => number {
  const context = getMeasureContext();
  if (context) {
    context.font = getMeasureFont();
    return (text) => context.measureText(text).width;
  }
  return approximateTextWidth;
}

function getMeasureContext(): CanvasRenderingContext2D | null {
  if (measureContext !== undefined) {
    return measureContext;
  }
  if (typeof document === 'undefined') {
    measureContext = null;
    return measureContext;
  }
  const canvas = document.createElement('canvas');
  measureContext = canvas.getContext('2d');
  return measureContext;
}

function getMeasureFont(): string {
  if (typeof document === 'undefined' || typeof getComputedStyle !== 'function') {
    return '13px monospace';
  }
  const fontElement = document.querySelector('.monaco-editor') || document.body;
  return getComputedStyle(fontElement).font || '13px monospace';
}

function scheduleSuggestWidgetLayout(editor: monaco.editor.ICodeEditor | null | undefined, width: number): void {
  if (!editor) {
    return;
  }
  const schedule = (callback: () => void) => {
    if (typeof window === 'undefined') {
      return;
    }
    if (typeof window.requestAnimationFrame === 'function') {
      window.requestAnimationFrame(callback);
      return;
    }
    window.setTimeout(callback, 0);
  };

  const run = (remainingAttempts: number) => {
    schedule(() => {
      layoutSuggestWidget(editor, width);
      if (remainingAttempts <= 0) {
        return;
      }
      run(remainingAttempts - 1);
    });
  };
  run(4);
}

function layoutSuggestWidget(editor: monaco.editor.ICodeEditor, width: number): boolean {
  prepareSuggestWidgetForEditorBoundary(editor);
  const widget = getSuggestWidget(editor);
  const element = widget?.element;
  const currentHeight = element?.size?.height || 0;
  const currentWidth = element?.size?.width || 0;
  if (!widget || !element || currentHeight <= 0) {
    return false;
  }
  const maxWidth = getEditorSuggestMaxWidth(editor);
  const maxHeightBelow = getEditorSuggestMaxHeightBelow(editor);
  const preferredWidth = Math.min(maxWidth, width);
  const preferredHeight = maxHeightBelow ? Math.min(currentHeight, maxHeightBelow) : currentHeight;
  if (maxHeightBelow) {
    widget?._contentWidget?.setPreference?.(CONTENT_WIDGET_POSITION_BELOW);
  }
  if (currentWidth >= preferredWidth && currentWidth <= maxWidth && currentHeight === preferredHeight) {
    widget?._contentWidget?.layout?.();
    applySuggestWidgetHorizontalBoundary(editor, element.domNode);
    return true;
  }
  if (typeof widget._resize === 'function') {
    widget._resize(preferredWidth, preferredHeight);
    widget?._contentWidget?.layout?.();
    applySuggestWidgetHorizontalBoundary(editor, element.domNode);
    return true;
  }
  if (typeof element.layout === 'function') {
    element.layout(preferredHeight, preferredWidth);
    widget?._contentWidget?.layout?.();
    applySuggestWidgetHorizontalBoundary(editor, element.domNode);
    return true;
  }
  return false;
}

function getSuggestWidget(editor: monaco.editor.ICodeEditor): any {
  const controller = editor.getContribution?.('editor.contrib.suggestController') as any;
  return controller?.widget?.value;
}

function prepareSuggestWidgetForEditorBoundary(editor: monaco.editor.ICodeEditor | null | undefined): void {
  if (!editor) {
    return;
  }
  const widget = getSuggestWidget(editor);
  const contentWidget = widget?._contentWidget;
  if (!contentWidget) {
    return;
  }
  if (!contentWidget[BOUNDED_SUGGEST_WIDGET_KEY]) {
    contentWidget[ORIGINAL_ALLOW_EDITOR_OVERFLOW_KEY] = contentWidget.allowEditorOverflow;
    boundedSuggestWidgets.set(contentWidget, {
      editor,
      originalAllowEditorOverflow: contentWidget.allowEditorOverflow,
    });
  }
  contentWidget[BOUNDED_SUGGEST_WIDGET_KEY] = true;
  contentWidget.allowEditorOverflow = false;
  if (contentWidget._added) {
    editor.removeContentWidget(contentWidget);
    editor.addContentWidget(contentWidget);
    editor.layoutContentWidget(contentWidget);
  }
}

function getEditorSuggestMaxWidth(
  editor: monaco.editor.ICodeEditor | null | undefined,
): number {
  const editorNode = editor?.getDomNode();
  if (!editorNode) {
    return getViewportMaxWidth(DEFAULT_MIN_WIDTH);
  }
  const contentBounds = getEditorContentBounds(editor);
  if (!Number.isFinite(contentBounds.width) || contentBounds.width <= 0) {
    return getViewportMaxWidth(DEFAULT_MIN_WIDTH);
  }
  return getBoundedEditorSuggestMaxWidth(contentBounds.width);
}

export function getBoundedEditorSuggestMaxWidth(editorWidth: number): number {
  if (!Number.isFinite(editorWidth) || editorWidth <= 0) {
    return getViewportMaxWidth(DEFAULT_MIN_WIDTH);
  }
  const availableWidth = Math.max(220, Math.floor(editorWidth - DEFAULT_EDITOR_BOUNDARY_MARGIN * 2));
  const ratioWidth = Math.floor(availableWidth * DEFAULT_EDITOR_MAX_WIDTH_RATIO);
  const preferredMaxWidth = Math.max(DEFAULT_MIN_WIDTH, ratioWidth);
  return Math.max(220, Math.min(availableWidth, DEFAULT_EDITOR_ABSOLUTE_MAX_WIDTH, preferredMaxWidth));
}

function getEditorSuggestMaxHeightBelow(editor: monaco.editor.ICodeEditor): number | null {
  const editorNode = editor.getDomNode();
  const position = editor.getPosition();
  const cursor = position ? editor.getScrolledVisiblePosition(position) : null;
  if (!editorNode || !cursor) {
    return null;
  }
  const editorRect = editorNode.getBoundingClientRect();
  const viewportBottom = typeof window === 'undefined'
    ? editorRect.bottom
    : Math.min(editorRect.bottom, window.innerHeight);
  const cursorBottom = editorRect.top + cursor.top + cursor.height;
  const availableHeight = Math.floor(viewportBottom - cursorBottom - DEFAULT_EDITOR_BOUNDARY_MARGIN);
  return availableHeight >= DEFAULT_MIN_BELOW_HEIGHT ? availableHeight : null;
}

function applySuggestWidgetHorizontalBoundary(editor: monaco.editor.ICodeEditor, widgetNode?: HTMLElement): void {
  const editorNode = editor.getDomNode();
  if (!editorNode || !widgetNode) {
    return;
  }
  widgetNode.style.transform = '';
  const contentBounds = getEditorContentBounds(editor);
  const widgetRect = widgetNode.getBoundingClientRect();
  const viewportWidth = typeof window === 'undefined' ? contentBounds.right : window.innerWidth;
  const leftLimit = Math.max(
    DEFAULT_EDITOR_BOUNDARY_MARGIN,
    contentBounds.left + DEFAULT_EDITOR_BOUNDARY_MARGIN,
  );
  const rightLimit = Math.min(
    viewportWidth - DEFAULT_EDITOR_BOUNDARY_MARGIN,
    contentBounds.right - DEFAULT_EDITOR_BOUNDARY_MARGIN,
  );
  let offset = getSuggestWidgetHorizontalOffset(widgetRect, leftLimit, rightLimit);
  if (offset) {
    offset = Math.round(offset);
    widgetNode.style.transform = `translateX(${offset}px)`;
  }
}

export function getSuggestWidgetHorizontalOffset(
  widgetRect: Pick<DOMRect, 'left' | 'right' | 'width'>,
  leftLimit: number,
  rightLimit: number,
): number {
  if (rightLimit <= leftLimit) {
    return 0;
  }
  let offset = 0;
  if (widgetRect.width > rightLimit - leftLimit) {
    offset = leftLimit - widgetRect.left;
  } else if (widgetRect.right > rightLimit) {
    offset = rightLimit - widgetRect.right;
  }
  if (widgetRect.left + offset < leftLimit) {
    offset += leftLimit - (widgetRect.left + offset);
  }
  return offset;
}

function getEditorContentBounds(editor: monaco.editor.ICodeEditor): Pick<DOMRect, 'left' | 'right' | 'width'> {
  const editorNode = editor.getDomNode();
  const editorRect = editorNode?.getBoundingClientRect();
  if (!editorRect) {
    return { left: 0, right: 0, width: 0 };
  }
  const layoutInfo = editor.getLayoutInfo?.();
  const contentLeft = Number.isFinite(layoutInfo?.contentLeft) ? layoutInfo.contentLeft : 0;
  const contentWidth = Number.isFinite(layoutInfo?.contentWidth)
    ? layoutInfo.contentWidth
    : Math.max(0, editorRect.width - contentLeft);
  const left = editorRect.left + contentLeft;
  const right = Math.min(editorRect.right, left + contentWidth);
  return {
    left,
    right,
    width: Math.max(0, right - left),
  };
}

function approximateTextWidth(text: string): number {
  return Array.from(text).reduce((width, char) => width + (char.charCodeAt(0) > 255 ? 14 : 8), 0);
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}
