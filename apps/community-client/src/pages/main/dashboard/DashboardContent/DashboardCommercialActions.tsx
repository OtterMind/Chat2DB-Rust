import AIButton from '@/blocks/AI/components/AIButton';
import { runtimeEditionConfig } from '@/constants/runtimeEdition';
import i18n from '@/i18n';
import { useAIStore } from '@/store/ai';
import { useGlobalStore } from '@/store/global';
import { useUserStore } from '@/store/user';
import { OrganizationType } from '@/typings/enterprise/organization';
import { copyToClipboard } from '@/utils';
import { IconButton, staticMessage } from '@chat2db/ui';
import { memo } from 'react';

interface IProps {
  dashboardId?: number | string;
}

export default memo<IProps>((props) => {
  const { dashboardId } = props;
  const { appUrlConfig } = useGlobalStore((state) => ({
    appUrlConfig: state.appUrlConfig,
  }));

  const { curUser } = useUserStore((state) => ({
    curUser: state.curUser,
  }));

  const handleShare = () => {
    if (!dashboardId) return;
    staticMessage.success(i18n('dashboard.share.linkCopied'));
    copyToClipboard(`${appUrlConfig.CHAT2DB_APP_URL}/dashboard/share/${dashboardId}`);
  };

  return (
    <>
      {runtimeEditionConfig.dashboardShare && curUser?.currentOrganization?.type !== OrganizationType.PERSONAL && (
        <IconButton code="icon-share" title="share" size="md" onClick={handleShare} />
      )}
      {runtimeEditionConfig.dashboardHostedAiGenerate && (
        <AIButton
          size="md"
          onClick={() => {
            useAIStore.getState().togglePanel();
          }}
        />
      )}
    </>
  );
});
