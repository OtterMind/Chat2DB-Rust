import { useState } from 'react';
import SplitPane from 'react-split-pane';
import { useStyles } from './style';
import PluginMenuList from './PluginMenuList';
import PluginItem from './PluginItem';
import { IPluginItem } from '@/typings/plugin';

const Plugin = () => {
  const { styles } = useStyles();
  const [curPlugin, setCurPlugin] = useState<IPluginItem>();

  return (
    <>
      <SplitPane className={styles.container} size={280} minSize={260} maxSize={400} split="vertical" primary="first">
        <PluginMenuList key={'1'} curPlugin={curPlugin} onClick={(plugin) => setCurPlugin(plugin)} />
        <PluginItem key={'2'} plugin={curPlugin} />
      </SplitPane>
    </>
  );
};
export default Plugin;
