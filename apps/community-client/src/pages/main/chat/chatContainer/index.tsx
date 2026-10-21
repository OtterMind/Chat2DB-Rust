import { memo, useCallback, useEffect, useRef, useState } from 'react';
import { useStyles } from './style';
import { AIChatHeaderInPanel } from '@/blocks/AI/components/AIChatHeader';
import ChatList from '../chatList';
import { useChatStore } from '@/store/chat';
import AIChatInput, { SendParams, ChatInputPropsRef } from '@/blocks/AI/components/AIChatInput';
import { AnswerPartsStatus, AnswerPartsType } from '@/constants/chat';
import { Empty, EmptyImage } from '@chat2db/ui';
import Spin from '@/components/Spin';
import i18n from '@/i18n';
import useSSERequest, { SSERequestStatus } from '@/hooks/useSSERequest';
import useSyncState from '@/hooks/useSyncState';
import { v4 as uuid } from 'uuid';
import { useWorkspaceStore } from '@/store/workspace';
import { IAICascaderData } from '@/blocks/AI/components/AICascaderSource';
import { formatTableString } from '@/utils/tableSchema';
import { useGlobalStore } from '@/store/global';
import { useAIStore } from '@/store/ai';

interface IProps {
  className?: string;
  /** Whether this is the Chat page. */
  isPage?: boolean;
}

const INIT_PART_CNT = -1;
const INIT_PART_ID_MAP = {};

export default memo<IProps>((props) => {
  const { className, isPage } = props;
  const { styles, cx } = useStyles();
  const [, setContent] = useState('');
  const [contextInfo, setContextInfo] = useState<IAICascaderData | null>(null);
  const chatInputRef = useRef<ChatInputPropsRef>(null);
  const hasInitializedRef = useRef(false);
  const [, setQuestionId, getQuestionId] = useSyncState<any>(null);
  const [answerId, setAnswerId] = useState<any>(null);
  const partCntRef = useRef(INIT_PART_CNT);
  const partIdMapRef = useRef<Record<string, number>>(INIT_PART_ID_MAP);
  const [trigger, setTrigger] = useState(0);
  const [forcibleTrigger, setForcibleTrigger] = useState(0);

  const dataSourceInfoRef = useRef({});
  const { mainPageActiveTab } = useGlobalStore((state) => {
    return {
      mainPageActiveTab: state.mainPageActiveTab,
    };
  });
  const {
    currentChat,
    updateInitChatInfo,
    createQuestion,
    appendAnswerParts,
    setHandleSend,
    updateAnswerId,
    createFakeNewChat,
    stopAllAnswerParts,
  } = useChatStore((state) => {
    return {
      currentChat: state.currentChat,
      createQuestion: state.createQuestion,
      appendAnswerParts: state.appendAnswerParts,
      setHandleSend: state.setHandleSend,
      updateAnswerId: state.updateAnswerId,
      updateInitChatInfo: state.updateInitChatInfo,
      createFakeNewChat: state.createFakeNewChat,
      stopAllAnswerParts: state.stopAllAnswerParts,
    };
  });

  const { cascaderDataMap } = useAIStore((state) => ({
    cascaderDataMap: state.cascaderDataMap,
  }));

  const { workspaceTabList, activeConsoleId } = useWorkspaceStore((state) => ({
    workspaceTabList: state.workspaceTabList,
    activeConsoleId: state.activeConsoleId,
  }));

  useEffect(() => {
  // When displayed on the Workspace page.
    const boundInfo = workspaceTabList?.find((tab) => tab.id === activeConsoleId)?.uniqueData ?? {};

    if (boundInfo && mainPageActiveTab === 'workspace' && cascaderDataMap['workspace']?.dataSourceId) {
      setContextInfo({
        dataSourceId: boundInfo?.dataSourceId,
        databaseName: boundInfo?.databaseName,
        schemaName: boundInfo?.schemaName,
      });
    }
  }, [workspaceTabList, activeConsoleId]);

  const {
    status,
    request: sseRequest,
    stop,
  } = useSSERequest(
    {
      baseURL: '/api/v2/ai/chat',
      lang: useGlobalStore.getState().baseSetting.language,
    },
    (parsedData) => {
  // Initialize the chat.
      if (
        !currentChat?.[mainPageActiveTab]?.id &&
        parsedData?.chatId &&
        parsedData?.title &&
        !hasInitializedRef.current
      ) {
        hasInitializedRef.current = true;
        updateInitChatInfo({
          id: parsedData?.chatId,
          title: parsedData?.title,
        });
      }

  // Retrieve data source information.
      if (parsedData.type === AnswerPartsType.DATABASE_INFO) {
        try {
          const res = JSON.parse(parsedData.content);
          dataSourceInfoRef.current = res;
        } catch {
          console.log('json_parse_error');
        }
        return;
      }

      if (parsedData.type === AnswerPartsType.LOADING) {
        setContent('');
        const { id, content } = parsedData || {};

        partCntRef.current++;
        partIdMapRef.current[parsedData.id] = partCntRef.current;
        appendAnswerParts({
          data: {
            id,
            status: AnswerPartsStatus.LOADING,
            // text: parsedData.content,
            step: partCntRef.current,
            loadingText: content,
          },
          questionId: getQuestionId(),
        });

        return;
      }

      if (parsedData.type === AnswerPartsType.LOADING_COMPLETE) {
        const step = partIdMapRef.current[parsedData.id];
        const { id, content } = parsedData || {};

        appendAnswerParts({
          data: {
            id,
            status: AnswerPartsStatus.FINISH,
            step,
            loadingText: content,
          },
          questionId: getQuestionId(),
        });

        setContent('');

        return;
      }

      if (parsedData.type === AnswerPartsType.TABLE) {
        try {
          const { tableStr } = formatTableString(parsedData.content);

          appendAnswerParts({
            data: {
              partType: AnswerPartsType.MARKDOWN,
              text: tableStr,
              status: AnswerPartsStatus.LOADING,
              step: partCntRef.current,
            },
            questionId: getQuestionId(),
          });
        } catch {
          console.log('json_parse_error');
        }
        return;
      }

      if (parsedData.type === AnswerPartsType.MARKDOWN || parsedData.type === AnswerPartsType.ERROR) {
        setContent((prev) => {
          let newContent = prev;
          if (parsedData.type === AnswerPartsType.ERROR) {
            newContent += `\n\n ❗${parsedData.content} \n\n`;
          } else {
            newContent += parsedData.content;
          }

          appendAnswerParts({
            data: {
              partType: AnswerPartsType.MARKDOWN,
              text: newContent,
              status: AnswerPartsStatus.LOADING,
              step: partCntRef.current,
              databaseInfo: dataSourceInfoRef.current,
            },
            questionId: getQuestionId(),
          });
          return newContent;
        });
        return;
      }

      if (parsedData.type === AnswerPartsType.DASHBOARD) {
        try {
          const res = JSON.parse(parsedData.content);
          appendAnswerParts({
            data: {
              ...(res || {}),
              // id: res.answerPartId,
              partType: AnswerPartsType.DASHBOARD,
              status: AnswerPartsStatus.LOADING,
              step: partCntRef.current,
            },
            questionId: getQuestionId(),
          });
        } catch {
          console.log('json_parse_error');
        }
        return;
      }

      if (parsedData.type === null) {
        setAnswerId(parsedData.answerId);
        return;
      }

      setTrigger((prev) => prev + 1);
    },
  );

  useEffect(() => {
    if (status === SSERequestStatus.FINISH) {
      setContent('');
      partCntRef.current = INIT_PART_CNT;
      partIdMapRef.current = INIT_PART_ID_MAP;
      const questionId = getQuestionId();
      stopAllAnswerParts(questionId);
      updateAnswerId(questionId, answerId!);
    }
  }, [status]);

  // Send the message.
  const handleSend = (params: SendParams) => {
    const { input, questionType } = params;
    const _questionId: any = uuid();
    setQuestionId(_questionId);
    createQuestion({
      questionId: _questionId,
      content: input,
      type: questionType,
    });
    setForcibleTrigger((prev) => prev + 1);
    sseRequest({
      chatId: currentChat?.[mainPageActiveTab]?.id,
      ...params,
    });
    hasInitializedRef.current = false;
  };

  const renderEmpty = () => {
    return (
      <div className={styles.emptyBox}>
        <Empty
          image={EmptyImage.Chat}
          title={i18n('chat.page.empty.text')}
          buttonText={i18n('chat.page.empty.button')}
          onButtonClick={() => {
            createFakeNewChat();
          }}
        />
      </div>
    );
  };

  const handleStop = () => {
    stop();
  };

  const handleContextChange = useCallback(() => {
    stop();
    hasInitializedRef.current = false;
    dataSourceInfoRef.current = {};
    partCntRef.current = INIT_PART_CNT;
    partIdMapRef.current = INIT_PART_ID_MAP;
    setAnswerId(null);
    setContent('');
    createFakeNewChat();
  }, [createFakeNewChat, stop]);

  useEffect(() => {
    if (chatInputRef.current?.triggerSend) {
      setHandleSend(chatInputRef.current?.triggerSend);
    }
  }, [currentChat?.[mainPageActiveTab]]);

  return (
    <div className={styles.chatContainerBox}>
      <Spin empty={renderEmpty()} isLoading={currentChat === null} isEmpty={currentChat === undefined}>
        {isPage ? null : <AIChatHeaderInPanel />}
        <ChatList forcibleTrigger={forcibleTrigger} trigger={trigger} />
        <div className={cx(styles.chatInputContainer, className)}>
          <AIChatInput
            ref={chatInputRef}
            className={styles.innerInput}
            contextInfo={contextInfo}
            dataSourceCollectionId={currentChat?.[mainPageActiveTab]?.dataSourceCollectionId}
            loading={status === SSERequestStatus.LOADING}
            onContextChange={handleContextChange}
            onChatSend={handleSend}
            onStop={handleStop}
            autoSize={{ minRows: 2, maxRows: 10 }}
          />
          {/* <div className={styles.aiCommonThink}>{i18n('chat.ai.common.think')}</div> */}
        </div>
      </Spin>
    </div>
  );
});
