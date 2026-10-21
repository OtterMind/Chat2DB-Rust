import { Fragment, memo, useMemo } from 'react';
import ChatItem from '@/blocks/Chat/ChatItem';
import { useChatStore } from '@/store/chat';
import { getChatDetailById } from '@/store/chat/slices/chatDetails/selectors';
import MessageRender from '@/blocks/Chat/MessageRender';
import { AnswerPartsType } from '@/constants/chat';
import RenderMarkdown from '@/blocks/Chat/MessageRender/RenderMarkdown';
import { AnswerParts } from '@/typings/chat';
import { answersContentToParts } from './dataTreating';

export type IProps = {
  id: string;
};

const ChatListItem = (props: IProps) => {
  const { id } = props;
  // Do not destructure { question, answers }; doing so recalculates them on every render.
  const question = useChatStore((state) => getChatDetailById(id)(state).question);
  const answers = useChatStore((state) => getChatDetailById(id)(state).answers);

  const isLast = useChatStore((state) => state?.chatDetailsIds?.[state?.chatDetailsIds.length - 1] === id);

  const { content: questionContent } = question || {};

  const questionParts: AnswerParts = useMemo(() => {
    return {
      partType: AnswerPartsType.MARKDOWN,
      text: questionContent,
    };
  }, [questionContent]);

  if (!question) {
    return;
  }

  return (
    <>
      <ChatItem renderMessage={() => <RenderMarkdown part={questionParts} />} avatar="user" type="question" />
      {answers?.map((item, index) => {
        // let { parts, content } = item || {};
        // const isLast = id === chatDetailsIds?.[chatDetailsIds.length - 1];

        const parts = answersContentToParts(item);

        return (
          <Fragment key={index}>
            <ChatItem
              type="answer"
              renderMessage={() => (
                <MessageRender last={isLast} answer={item} questionType={question.type} parts={parts} />
              )}
            />
          </Fragment>
        );
      })}
    </>
  );
};

export default memo(ChatListItem);
