import { IPluginItem } from '@/typings/plugin';
import { useStyles } from './style';
import { Flex } from 'antd';
import i18n from '@/i18n';

interface PluginMenuItemProps {
  plugin: IPluginItem;
  isActive: boolean;
  onClick?: () => void;
  onButtonClick?: () => void;
}

const PluginMenuItem = ({ plugin, isActive, onClick, onButtonClick }: PluginMenuItemProps) => {
  const { styles } = useStyles({ isActive });

  const renderDownloadButton = () => {
    return (
      <div
        className={styles.downloadButton}
        onClick={(e) => {
          e.stopPropagation();
          onButtonClick && onButtonClick();
        }}
      >
        {i18n('plugin.download')}
      </div>
    );
  };

  return (
    <div className={styles.container} onClick={onClick}>
      <div className={styles.icon}>
        <img src={plugin.icon} alt={plugin.name} />
      </div>
      <Flex justify="space-between" align="center" flex={1}>
        <Flex vertical>
          <div className={styles.title}>{plugin.name}</div>
          <div className={styles.description}>{plugin.description}</div>
        </Flex>

        {renderDownloadButton()}
      </Flex>
    </div>
  );
};

export default PluginMenuItem;
