import React, { memo } from 'react';
import { useStyles } from './style';

export interface ChatItemProps {
  className?: string;
  avatar?: 'user';
  type?: 'question' | 'answer';
  // the last message?
  renderMessage: () => React.ReactNode;
  // renders custom tail
  renderFooter?: () => React.ReactNode;
}

export default memo<ChatItemProps>((props) => {
  const { className, renderMessage, renderFooter, type } = props;
  const { styles, cx } = useStyles();

  return (
    <div
      className={cx(
        styles.container,
        { [styles.questionContainer]: type === 'question' },
        { [styles.answerContainer]: type === 'answer' },
        className,
      )}
    >
      <div className="messageContainer">
        <div className="timeBox" />
        <div className="messageContent">
          <div className="messageBox">
            {renderMessage()}
            {renderFooter?.()}
          </div>
        </div>
        {/* {renderFooter?.()} */}
      </div>
    </div>
  );
});
