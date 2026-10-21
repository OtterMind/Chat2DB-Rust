import { useEffect } from 'react';
import classnames from 'classnames';
import i18n from '@/i18n';
import { extendConfig } from '../config';
import { IconButton } from '@chat2db/ui';
import { useWorkspaceStore } from '@/store/workspace';
import { useImportExportStore } from '@/store/importExport';
import { useStyles } from './style';
import { canImportExport } from '@/utils/env';
import { Divider } from 'antd';
import { useAIStore } from '@/store/ai';
import AIButton from '@/blocks/AI/components/AIButton';

interface IToolbar {
  code: string;
  title: string;
  icon: any;
  components: any;
}

interface IProps {
  className?: any;
}

export default (props: IProps) => {
  const { className } = props;
  const { styles } = useStyles();
  const { currentWorkspaceExtend, setCurrentWorkspaceExtend } = useWorkspaceStore((state) => {
    return {
      currentWorkspaceExtend: state.currentWorkspaceExtend,
      setCurrentWorkspaceExtend: state.setCurrentWorkspaceExtend,
    };
  });
  const { showExportToolbar, setShowExportToolbar } = useImportExportStore((state) => {
    return {
      showExportToolbar: state.showExportToolbar,
      setShowExportToolbar: state.setShowExportToolbar,
    };
  });
  const { showPanel: showAIPanel } = useAIStore((state) => ({
    showPanel: state.showPanel,
  }));

  const changeExtend = (item: IToolbar) => {
    if (currentWorkspaceExtend === item.code) {
      setCurrentWorkspaceExtend(null);
      return;
    }
    setCurrentWorkspaceExtend(item.code);
  };

  useEffect(() => {
    useWorkspaceStore.getState().togglePanelRight(!!currentWorkspaceExtend || showAIPanel);
  }, [currentWorkspaceExtend, showAIPanel]);

  return (
    <div className={classnames(className, styles.workspaceExtendNav)}>
      <div className={styles.topBox}>
        {extendConfig.map((item, index) => {
          return (
            <IconButton
              size="lg"
              key={index}
              title={item.title}
              tooltipPlacement="left"
              code={item.icon}
              isActive={currentWorkspaceExtend === item.code}
              onClick={() => {
                changeExtend(item);
                useAIStore.getState().setShowPanel(false);
              }}
            />
          );
        })}
        <Divider style={{ margin: '8px 0px' }} />

        <AIButton
          onClick={() => {
            setCurrentWorkspaceExtend(null);
            useAIStore.getState().togglePanel();
          }}
        />
      </div>
      <div className={styles.bottomBox}>
        {canImportExport && (
          <IconButton
            size="lg"
            title={i18n('workspace.title.exportProgressBar')}
            tooltipPlacement="left"
            code="icon-export-details"
            isActive={showExportToolbar}
            onClick={() => setShowExportToolbar(!showExportToolbar)}
          />
        )}

        {/* <Tooltip title={i18n('workspace.title.ai')} placement="left">
        </Tooltip> */}
      </div>
    </div>
  );
};
