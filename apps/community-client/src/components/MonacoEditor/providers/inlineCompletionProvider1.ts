import { ChatSourceType } from '@/constants/chat';
import monacoTypes from 'monaco-editor/esm/vs/editor/editor.api';
import MagicStickService from '@/service/magicStick';
import { IMonacoEditorProps } from '..';
import { isValid } from '@/utils/check';

export class InLineCompletionProvider implements monacoTypes.languages.InlineCompletionsProvider {
  props: IMonacoEditorProps;
  monaco: any;
  fetchController: AbortController | null;
  isActive: boolean;

  constructor(props: any, monaco: any) {
    this.props = props;
    this.monaco = monaco;
    this.fetchController = null;
    this.isActive = false;
  }

  async provideInlineCompletions(
    model: monacoTypes.editor.ITextModel,
    position: monacoTypes.Position,
    context: monacoTypes.languages.InlineCompletionContext,
    token: monacoTypes.CancellationToken,
  ): Promise<monacoTypes.languages.InlineCompletions<monacoTypes.languages.InlineCompletion> | null | undefined> {

    if (this.isActive) {
        // Return an empty result while another InlineCompletionsProvider is running.
      return { items: [] };
    }

    if (this.fetchController) {
      this.fetchController.abort();
    }

    if (token.isCancellationRequested) {
      return;
    }

    if (!isValid(this.props.boundInfo.dataSourceId)) {
      return;
    }

    // get text before the position of the completion
    const word = model.getValueInRange({
      startLineNumber: 1,
      startColumn: 1,
      endLineNumber: position.lineNumber,
      endColumn: position.column,
    });

        // Return unless the last character is a space, newline, or period.
    if (!word.endsWith(' ')) {
      return;
    }

    const trimWord = word.trim();
        // Return when the last character is a semicolon.
    if (trimWord.endsWith(';')) {
      return;
    }

        // beforeContext: content after the last semicolon between the start and the cursor.
    const beforeContext = word.trimStart().split(';')
.pop() || '';
    if (beforeContext.length < 3) {
      return;
    }

        // afterContext: content before the first semicolon between the cursor and the end.

        // From the cursor to the end of the text.
    const afterContext =
      model
        .getValueInRange({
          startLineNumber: position.lineNumber,
          startColumn: position.column,
          // endLineNumber: position.lineNumber,
          // endColumn: model.getLineMaxColumn(position.lineNumber),
          endLineNumber: model.getLineCount(),
          endColumn: model.getLineMaxColumn(model.getLineCount()),
        })
        .trim()
        .split(';')[0] + ';';

    console.log('beforeContext', beforeContext);
    console.log('afterContext', afterContext);
    this.isActive = true;
    this.fetchController = new AbortController();
    let item: monacoTypes.languages.InlineCompletion = {
      insertText: '',
    };
    let flagError = false;
    try {
      const generatedText = await MagicStickService.queryPrompt(
        {
          dataSourceId: this.props.boundInfo.dataSourceId!,
          databaseName: this.props.boundInfo.databaseName,
          schemaName: this.props.boundInfo.schemaName,
          source: ChatSourceType.DATASOURCE_CHAT,
          beforeContext,
          afterContext,
        },
        {
          signal: this.fetchController.signal,
        },
      );
      console.log('generatedText', generatedText);

      item = {
        insertText: generatedText,
        // The range spans from the cursor to the next semicolon or end of text.
        range: {
          startLineNumber: position.lineNumber,
          startColumn: position.column,
          endLineNumber: position.lineNumber,
          endColumn: model.getLineMaxColumn(position.lineNumber),
        },
      };
    } catch (error) {
      console.log('error', error);
      flagError = true;
    }

    this.isActive = false;
    if (flagError) {
      return;
    }

    // abort if there is a signal
    if (token.isCancellationRequested) {
      return;
    }

    console.log('items', [item]);
    return {
      items: [item],
      enableForwardStability: true,
    };
  }

  handleItemDidShow(
    _completions: monacoTypes.languages.InlineCompletions<monacoTypes.languages.InlineCompletion>,
    _item: monacoTypes.languages.InlineCompletion,
    _updatedInsertText: string,
  ): void {}

  handlePartialAccept(
    _completions: monacoTypes.languages.InlineCompletions<monacoTypes.languages.InlineCompletion>,
    _item: monacoTypes.languages.InlineCompletion,
    _acceptedCharacters: number,
  ): void {}

  freeInlineCompletions(
    _completions: monacoTypes.languages.InlineCompletions<monacoTypes.languages.InlineCompletion>,
  ): void {}

  toString?(): string {
    throw new Error('Method not implemented.');
  }
}
