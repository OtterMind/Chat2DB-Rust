import { memo, useMemo, Fragment } from 'react';
import { AnswerPartsType, QuestionType } from '@/constants/chat';
import { ChartType } from '@/constants/dashboard';
import RenderChartCardBox from './RenderChartCardBox';
import RenderMarkdown from './RenderMarkdown';
import { Flex } from 'antd';
import Analyzing from './Analyzing';
import { AnswerParts, AnswerVO } from '@/typings/chat';
import { formatTableString } from '@/utils/tableSchema';

export interface MessageRenderProps {
  parts?: AnswerParts[];
  questionType: QuestionType;
  answer: AnswerVO;
  last?: boolean;
}

const MessageRender = (props: MessageRenderProps) => {
  const { parts, last } = props;

  const renderStepAnalyzing = (item: AnswerParts) => {
    const renderContent = () => {
      if ([AnswerPartsType.DASHBOARD, AnswerPartsType.DATA].includes(item.partType)) {
        if (item.partType === AnswerPartsType.DATA) {
          item.chartSchema = {
            ...item.chartSchema,
            chartType: ChartType.Table,
          };
        }
        return <RenderChartCardBox parts={item} />;
      }

      return <RenderMarkdown part={item} />;
    };

    return <Analyzing parts={item}>{renderContent()}</Analyzing>;
  };

  const contentList = useMemo(() => {
    return (parts || [])?.map((item, index) => {
      if (item.partType === AnswerPartsType.TABLE) {
        const { tableStr } = formatTableString(item.tableMap || '');
        item.text = tableStr;

        return <Fragment key={index}>{renderStepAnalyzing(item)}</Fragment>;
      }

      if (item.partType !== AnswerPartsType.RECOMMEND_QUESTION) {
        return <Fragment key={index}>{renderStepAnalyzing(item)}</Fragment>;
      }
    });
  }, [parts, last]);

  return (
    <Flex vertical gap={6}>
      {contentList}
    </Flex>
  );
};

export default memo(MessageRender);
