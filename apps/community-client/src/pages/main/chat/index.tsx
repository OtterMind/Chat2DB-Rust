import { FC } from 'react';
import ChatMenuList from './chatMenuList';
import ChatContainer from './chatContainer';
import { getAllUrlParams } from '@/utils/url';
import { useChatStore } from '@/store/chat';
import ChatShare from './chatShare';
import SplitPane from 'react-split-pane';

export interface AIChatProps {}

const AIChat: FC<AIChatProps> = () => {
  const { share_id, share_type } = getAllUrlParams();
  const share = {
    id: share_id,
    type: share_type,
  };

  const { isOpenShare, shareChatVO, setOpenShareModal } = useChatStore((state) => ({
    isOpenShare: state.isOpenShare,
    shareChatVO: state.shareChatVO,
    setOpenShareModal: state.setOpenShareModal,
  }));

  return (
    <>
      {share_id ? (
        <ChatContainer share={share} />
      ) : (
        <SplitPane
          size={220}
          pane2Style={{ width: '0px' }}
          minSize={180}
          maxSize={400}
          split="vertical"
          primary="first"
        >
          <ChatMenuList />
          <ChatContainer isPage />
        </SplitPane>
      )}
      {/* Share dialog. */}
      <ChatShare
        chatItem={shareChatVO}
        open={isOpenShare}
        onCancel={() => setOpenShareModal(false)}
        onOk={() => setOpenShareModal(false)}
      />
    </>
  );
};

export default AIChat;
