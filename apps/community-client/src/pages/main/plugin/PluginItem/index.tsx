import { useStyles } from './style';
import { Flex, TabsProps, Tabs } from 'antd';
import { IconfontSvg, Markdown } from '@chat2db/ui';
import { IPluginItem } from '@/typings/plugin';
import UsedContent from './used';
import i18n from '@/i18n';
import { openWebPage } from '@/utils/url';

interface PluginItemProps {
  plugin?: IPluginItem;
}

const PluginItem = ({ plugin }: PluginItemProps) => {
  const { styles } = useStyles();
  if (!plugin) {
    return;
  }

  const items: TabsProps['items'] = [
    { key: 'intro', label: i18n('plugin.item.intro'), children: <Markdown content={plugin.detail} /> },
    { key: 'usage', label: i18n('plugin.item.usage'), children: <UsedContent token={plugin.token} /> },
  ];

  return (
    <div className={styles.wrapper}>
      <div className={styles.top}>
        <Flex gap={24}>
          <img className={styles.image} src={plugin.icon} />
          <Flex vertical gap={12}>
            <Flex gap={12}>
              <a
                className={styles.title}
                onClick={() => {
                  openWebPage(plugin.url);
                }}
              >
                {plugin.name}
              </a>
              <div className={styles.version}>V{plugin.version}</div>
            </Flex>
            <div className={styles.desc}>{plugin.description}</div>
            <div className={styles.downCount}>
              <IconfontSvg code="icon-download" size={18} />
              {plugin.downCount}
            </div>
          </Flex>
        </Flex>
        <div className={styles.button} />
      </div>

      <div className={styles.content}>
        <Tabs items={items} destroyInactiveTabPane />
      </div>
    </div>
  );
};

export default PluginItem;
