import { memo, useMemo, useState } from 'react';
import { useStyles } from './style';
import { IconButton } from '@chat2db/ui';
import { AnswerVO } from '@/typings/chat';
import chatService from '@/service/chat';

interface IProps {
  className?: string;
  answer: AnswerVO;
}

export default memo<IProps>((props) => {
  const { className, answer } = props;
  const { styles, cx } = useStyles();
  const [goodFeedback, setGoodFeedback] = useState<boolean>(!!answer.goodFeedback);
  const [badFeedback, setBadFeedback] = useState<boolean>(!!answer.badFeedback);

  const actionBarConfig = useMemo(() => {
    return [
      {
        icon: 'icon-like',
        label: 'like',
        isActive: goodFeedback,
        handleClick: () => {
          setGoodFeedback(!goodFeedback);
          setBadFeedback(false);
          chatService.chatFeedback({
            id: answer.id!,
            like: !goodFeedback,
            dislike: false,
          });
        },
      },
      {
        icon: 'icon-dislike',
        label: 'dislike',
        isActive: badFeedback,
        handleClick: () => {
          setBadFeedback(!badFeedback);
          setGoodFeedback(false);
          chatService.chatFeedback({
            id: answer.id!,
            like: false,
            dislike: !badFeedback,
          });
        },
      },
    ];
  }, [goodFeedback, badFeedback]);

  return (
    <div className={cx(styles.chartItemActionBar, className)}>
      {actionBarConfig.map((item, index) => {
        return (
          <IconButton
            key={index}
            size="sm"
            className={cx({ [styles.activeIcon]: item.isActive })}
            code={item.icon}
            onClick={item.handleClick}
          />
        );
      })}
    </div>
  );
});
