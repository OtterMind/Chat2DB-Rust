import { memo } from 'react';
import { useStyles } from './style';
import ZoerWorkspaceLeft from './ZoerWorkspaceLeft';
import WorkspaceTabs from '@/pages/main/workspace/components/WorkspaceTabs';
import { Splitter } from 'antd';
import { useZoerStore } from '@/store/zoer';

interface IProps {
  className?: string;
}

export default memo<IProps>((props) => {
  const { className } = props;
  const { styles, cx } = useStyles();

  const { zoerBoundInfo } = useZoerStore((state) => {
    return {
      zoerBoundInfo: state.zoerBoundInfo,
    };
  });

  if (!zoerBoundInfo) {
    return null;
  }

  return (
    <Splitter className={cx(styles.container, className)}>
      <Splitter.Panel defaultSize={300} min={200} max={400}>
        <ZoerWorkspaceLeft boundInfo={zoerBoundInfo} />
      </Splitter.Panel>
      <Splitter.Panel>
        <WorkspaceTabs />
      </Splitter.Panel>
    </Splitter>
  );
});
