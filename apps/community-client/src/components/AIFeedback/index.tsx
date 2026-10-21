import { Button, Flex } from 'antd';
import { useStyles } from './style';
import { FC } from 'react';
import i18n from '@/i18n';

export enum AIFeedbackType {
  ACCEPT = 'ACCEPT',
  GIVE_UP = 'GIVE_UP',
}

interface AIFeedbackProps {
  onFeedback: (type: AIFeedbackType) => void;
}

const AIFeedback: FC<AIFeedbackProps> = ({ onFeedback }) => {
  const { styles } = useStyles();

  const handleClick = (type: AIFeedbackType) => {
    onFeedback && onFeedback(type);
  };

  return (
    <Flex gap={16} vertical className={styles.container}>
      <div className={styles.tips}>{i18n('ai.feedback.tips')}</div>
      <Flex gap={8}>
        <Button size="middle" onClick={() => handleClick(AIFeedbackType.ACCEPT)} className={styles.acceptButton}>
          {i18n('ai.feedback.accept')}
        </Button>
        <Button size="middle" danger onClick={() => handleClick(AIFeedbackType.GIVE_UP)}>
          {i18n('ai.feedback.reject')}
        </Button>
      </Flex>
    </Flex>
  );
};

export default AIFeedback;
