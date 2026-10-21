import { memo, useMemo } from 'react';
import { useStyles } from './style';
import Lottie from 'lottie-react';
import loading_test_1 from '@/assets/lottieFiles/loading_test_1.json';
import { AnswerParts } from '@/typings/chat';
import { QuestionType } from '@/constants/chat';
import { useChatStore } from '@/store/chat';

interface IProps {
  className?: string;
  recommendQuestion?: AnswerParts;
  questionType: QuestionType;
}

export default memo<IProps>((props) => {
  const { className, recommendQuestion, questionType } = props;
  const { styles, cx } = useStyles();
  const { recommends } = recommendQuestion || {};
  const { handleSend } = useChatStore((state) => {
    return {
      handleSend: state.handleSend,
    };
  });

  const noNeedRecommendList = useMemo(() => {
    return [QuestionType.SQL_EXPLAIN, QuestionType.SQL_OPTIMIZER].includes(questionType);
  }, [questionType]);

  if (noNeedRecommendList) {
    return null;
  }

  return (
    <div className={cx(styles.renderRecommendList, className)}>
      {recommends ? (
        <div className={styles.recommendList}>
          {recommends?.map((item, index) => {
            return (
              <div
                key={index}
                className={styles.recommendListItem}
                onClick={() => {
                  handleSend?.({ value: item, questionType, matchTable: true });
                }}
              >
                {item}
              </div>
            );
          })}
        </div>
      ) : (
        <div style={{ width: '30px', height: '30px' }}>
          <Lottie animationData={loading_test_1} loop={true} autoplay={true} />
        </div>
      )}
    </div>
  );
});
