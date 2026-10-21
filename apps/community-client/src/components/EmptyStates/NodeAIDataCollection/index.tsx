import { memo } from 'react';
import { useStyles } from './style';
import i18n from '@/i18n';

interface IProps {
  className?: string;
}

export default memo<IProps>((props) => {
  const { className } = props;
  const { styles, cx } = useStyles();
  return (
    <div className={cx(styles.nodeAIDataCollection, className)}>
      <span>{i18n('workspace.aiDataCollection.noData')}</span>
      <span>{i18n('workspace.aiDataCollection.noData.tip')}</span>
      <a target="_blank" rel="noreferrer" href="https://docs.chat2db-ai.com/docs/ai-chat/ai-data-collection">
        {i18n('workspace.aiDataCollection.noData.doc')}
      </a>
    </div>
  );
});
