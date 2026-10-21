import { FC, useMemo } from 'react';
import AIChatHeader, { AIChatHeaderProps } from '.';
import { IconButton } from '@chat2db/ui';
import { Share2 } from 'lucide-react';
import i18n from '@/i18n';

export interface AIChatHeaderInPageProps extends AIChatHeaderProps {
  onClickShare?: () => void;
  canShare?: boolean;
}

export const AIChatHeaderInPage: FC<AIChatHeaderInPageProps> = ({ canShare, onClickShare }) => {
  const extraBtn = useMemo(() => {
    if (!canShare) return null;

    return <IconButton icon={Share2} title={i18n('chat.menu.share')} size="lg" onClick={onClickShare} />;
  }, []);

  return <AIChatHeader extraBtn={extraBtn} />;
};
