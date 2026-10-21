import { memo, useState } from 'react';
import { useStyles } from './style';
import NavList, { NavType } from './components/NavList';
import SwitchContainer from './components/SwitchContainer';

interface IProps {
  className?: string;
}

export default memo<IProps>((props) => {
  const { className } = props;
  const { styles, cx } = useStyles();
  const [menuKey, setMenuKey] = useState<NavType>(NavType.KNOWLEDGE_TERM);

  return (
    <div className={cx(styles.container, className)}>
      <NavList menuKey={menuKey} onClickMenu={(key) => setMenuKey(key)} />
      <SwitchContainer menuKey={menuKey} />
    </div>
  );
});
