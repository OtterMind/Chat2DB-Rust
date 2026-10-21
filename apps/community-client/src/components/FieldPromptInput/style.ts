import { createStyles } from 'antd-style';

export const useStyles = createStyles(
  ({ css, token }, { minWidth, maxLines }: { minWidth: number; maxLines?: number }) => {
    return {
    fieldPromptInput: css`
      display: flex;
      align-items: stretch;
      width: fit-content;
      max-width: 100%;
      min-height: 26px;
      border-radius: 4px;
      border: 1px solid ${token.colorBorder};
      box-sizing: border-box;
      .fieldPrompt {
        flex-shrink: 0;
        padding: 0px 7px;
        line-height: 24px;
        background: ${token.colorFillTertiary};
        color: ${token.colorTextTertiary};
        border-radius: 4px 0 0 4px;
      }
      .contentEditableSpan {
        flex: 1;
        /* all: unset; */
        padding: 0px 7px;
        width: fit-content;
        min-width: ${minWidth}px;
        box-sizing: border-box;
        word-wrap: break-word;
        line-height: 24px;
        overflow: auto;
        ${maxLines ? `max-height: ${maxLines * 24}px;` : ''}

        /* Remove the editable span's default styles. */
        margin: 0;
        padding: 0px 2px;
        border: none;
        outline: none;
        background: transparent;
      }
    `,
    fieldPromptInputActive: css`
      border: 1px solid ${token.colorPrimary};
      .fieldPrompt {
        background: ${token.colorPrimaryBgHover};
        color: ${token.colorPrimary};
      }
    `,
    };
  },
);
