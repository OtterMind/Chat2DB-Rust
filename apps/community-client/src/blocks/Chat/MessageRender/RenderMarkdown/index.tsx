import React, { memo, useMemo } from 'react';
import { Markdown, type MarkdownProps } from '@chat2db/ui';
import { useStyles } from './style';
import { AnswerParts } from '@/typings/chat';
import CodeBlock from '../CodeBlock';

export interface MessageContentProps {
  part: AnswerParts;
  componentProps?: MarkdownProps['componentProps'];
}

const MessageContent = (props: MessageContentProps) => {
  const { part, componentProps } = props;
  const { styles } = useStyles();

  const preComponent = useMemo(() => {
    return (preProps: any) => <CodeBlock {...preProps} {...componentProps?.pre} databaseInfo={part.databaseInfo} />;
  }, [componentProps?.pre, part.databaseInfo]);

  return (
    <div className={styles.message}>
      <Markdown
        content={part.text || ''}
        componentProps={componentProps}
        extraComponent={{
          pre: preComponent as unknown as React.ReactNode,
        }}
      />
    </div>
  );
};

export default memo(MessageContent);
