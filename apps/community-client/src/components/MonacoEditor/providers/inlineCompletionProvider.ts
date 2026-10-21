import { IBoundInfo } from '@/typings';
import * as monaco from 'monaco-editor/esm/vs/editor/editor.api';
import MagicStickService from '@/service/magicStick';
import { isNumber } from 'lodash';
import { ChatSourceType } from '@/constants/chat';

export class InlineCompletionsProvider implements monaco.languages.InlineCompletionsProvider {
  boundInfo: IBoundInfo;
  isActive: boolean;
  fetchController: AbortController | null;
  constructor({ boundInfo, isActive }) {
    this.boundInfo = boundInfo;
    this.isActive = isActive;
    this.fetchController = null;
  }

  setActive(isActive: boolean) {
    this.isActive = isActive;
  }

  setBoundInfo(boundInfo: IBoundInfo) {
    this.boundInfo = boundInfo;
  }


  /** Determine whether completions are needed. */
  checkNeedProvider() {
    // Do not provide completions while inactive.
    if (!this.isActive) {
      return false;
    }

    // Do not provide completions without a data source.
    if (!isNumber(this.boundInfo.dataSourceId)) {
      return false;
    }
    return true;
  }

  async provideInlineCompletions(
    model: monaco.editor.ITextModel,
    position: monaco.Position,
  ): Promise<monaco.languages.InlineCompletions<monaco.languages.InlineCompletion> | null | undefined> {
    if (!this.checkNeedProvider()) {
      return;
    }
    // console.log('Enter provideInlineCompletions', this.boundInfo.dataSourceId, this.isActive);

    // Trigger completions only on spaces.
    const textBeforePointer = this.getTextBeforePointer(model, position);
    if (!textBeforePointer.endsWith(' ')) {
      return;
    }

    if (this.fetchController) {
      this.fetchController.abort();
    }

    const beforeContext = this.getTextBeforeSemicolon(model, position);
    const afterContext = this.getTextAfterSemicolon(model, position);

    this.fetchController = new AbortController();
    const data = await MagicStickService.queryPrompt(
      {
        dataSourceId: this.boundInfo.dataSourceId,
        databaseName: this.boundInfo.databaseName,
        schemaName: this.boundInfo.schemaName,
        source: ChatSourceType.DATASOURCE_CHAT,
        beforeContext,
        afterContext,
      },
      {
        signal: this.fetchController.signal,
      },
    );

    const content = data?.content || '';

    return {
      items: [
        {
          range: {
            startLineNumber: position.lineNumber,
            startColumn: position.column,
            endLineNumber: position.lineNumber,
            endColumn: position.column,
          },
          insertText: content,
        },
      ],
    };
  }

  freeInlineCompletions(_completions: monaco.languages.InlineCompletions<monaco.languages.InlineCompletion>) {
    // Release resources.
  }

  /** Get the previous token, which often contains the user's matching character. */
  getLastToken(model: monaco.editor.ITextModel, position: monaco.Position) {
    const textBeforePointer = this.getTextBeforePointer(model, position);
    const tokens = textBeforePointer.trim().split(/\s+/);
    return tokens[tokens.length - 1].toLowerCase();
  }

  /** Get the penultimate token, which is normally the previous word. */
  getLastSecondToken(model: monaco.editor.ITextModel, position: monaco.Position) {
    const textBeforePointer = this.getTextBeforePointer(model, position);
    const tokens = textBeforePointer.trim().split(/\s+/);
    return (tokens[tokens.length - 2] || '').toLowerCase();
  }

  /** Text on the current line before the cursor. */
  getTextBeforePointer(model: monaco.editor.ITextModel, position: monaco.Position) {
    const { lineNumber, column } = position;
    return model.getValueInRange({
      startLineNumber: lineNumber,
      startColumn: 0,
      endLineNumber: lineNumber,
      endColumn: column,
    });
  }
  /** Text on all lines before the cursor. */
  getTextBeforePointerMulti(model: monaco.editor.ITextModel, position: monaco.Position) {
    const { lineNumber, column } = position;
    return model.getValueInRange({
      startLineNumber: 1,
      startColumn: 0,
      endLineNumber: lineNumber,
      endColumn: column,
    });
  }
  /** Text on all lines after the cursor. */
  getTextAfterPointerMulti(model: monaco.editor.ITextModel, position: monaco.Position) {
    const { lineNumber, column } = position;
    return model.getValueInRange({
      startLineNumber: lineNumber,
      startColumn: column,
      endLineNumber: model.getLineCount(),
      endColumn: model.getLineMaxColumn(model.getLineCount()),
    });
  }

  /** Text after the first semicolon before the cursor. */
  getTextBeforeSemicolon(model: monaco.editor.ITextModel, position: monaco.Position) {
    const { lineNumber, column } = position;
    const text = model.getValueInRange({
      startLineNumber: 1,
      startColumn: 0,
      endLineNumber: lineNumber,
      endColumn: column,
    });
    return text.split(';').pop() || '';
  }

  /** Text before the first semicolon after the cursor. */
  getTextAfterSemicolon(model: monaco.editor.ITextModel, position: monaco.Position) {
    const { lineNumber, column } = position;
    const text = model.getValueInRange({
      startLineNumber: lineNumber,
      startColumn: column,
      endLineNumber: model.getLineCount(),
      endColumn: model.getLineMaxColumn(model.getLineCount()),
    });
    return text.split(';').shift() || '';
  }

  /** Get the complete SQL around the cursor. */
  getSQLTextByPosition(model: monaco.editor.ITextModel, position: monaco.Position) {
    return this.getTextBeforeSemicolon(model, position) + this.getTextAfterSemicolon(model, position);
  }
}
