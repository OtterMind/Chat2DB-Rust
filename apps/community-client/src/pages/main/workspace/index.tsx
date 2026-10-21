import React, { memo, useEffect } from 'react';
import SplitPane from 'react-split-pane';
import { useWorkspaceStore } from '@/store/workspace';
import WorkspaceLeft from './components/WorkspaceLeft';
import WorkspaceRight from './components/WorkspaceRight';

import { useStyles } from './style';

const workspacePage = memo(() => {
  const { cx } = useStyles();
  const { panelLeftWidth, setPanelLeftWidth } = useWorkspaceStore((state) => {
    return {
      panelLeftWidth: state.layout.panelLeftWidth,
      setPanelLeftWidth: state.setPanelLeftWidth,
    };
  });
  const [size, setSize] = React.useState(panelLeftWidth);

  useEffect(() => {
    if (size !== panelLeftWidth) {
      setSize(panelLeftWidth);
    }
  }, [panelLeftWidth]);

  return (
    <>
      <SplitPane
        split="vertical"
        className={cx({ ['ResizerSizeIsZeroRight']: size === 0 })}
        pane1Style={{ zIndex: 2 }}
        pane2Style={{ zIndex: 1 }}
        onChange={(newSize) => {
          if (newSize < 100) {
            setSize(0);
            setPanelLeftWidth(0);
            return;
          }
          setSize(newSize);
          setPanelLeftWidth(newSize);
        }}
        size={size}
        minSize={0}
        primary="first"
      >
        <WorkspaceLeft />
        <WorkspaceRight />
      </SplitPane>
    </>
  );
});

export default workspacePage;
