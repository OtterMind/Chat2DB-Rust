import { memo, useMemo } from 'react';
import { useStyles } from './style';
import ChartCardBox from '@/blocks/BI/ChartCardBox';
import { AnswerPartsStatus } from '@/constants/chat';
import { AnswerParts } from '@/typings/chat';
import { useChatStore } from '@/store/chat';

interface IProps {
  parts: AnswerParts;
}

export default memo<IProps>((props) => {
  const { parts } = props;
  const { styles } = useStyles();
  const updateAnswerPartsToService = useChatStore((s) => s.updateAnswerPartsToService);

  const submitEditorChartCallback = (data) => {
    updateAnswerPartsToService({
      id: parts.id,
      ...data,
    });
  };

  const chartDetail = useMemo(() => {
    if (parts.status === AnswerPartsStatus.LOADING) {
      return null;
    }
    const _chartDetail = {
      databaseInfo: parts.databaseInfo,
      metaData: parts.metaData,
      chartSchema: parts.chartSchema,
    };
    return _chartDetail;
  }, [parts]);

  if (!chartDetail) {
    return null;
  }

  return (
    <ChartCardBox
      chartDetail={chartDetail}
      submitEditorChartCallback={submitEditorChartCallback}
      showDing
      className={styles.dashboardCard}
    />
  );
});
