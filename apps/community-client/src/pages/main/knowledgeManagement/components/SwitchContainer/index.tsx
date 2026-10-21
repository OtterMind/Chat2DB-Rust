import { memo } from 'react';
import { useStyles } from './style';
import Terminology from '../Terminology';
import { NavType } from '../NavList';
import AnnotationTable from '../AnnotationTable';

interface IProps {
  className?: string;
  menuKey: NavType;
}

export default memo<IProps>((props) => {
  const { className, menuKey } = props;
  const { styles, cx } = useStyles();

  const switchContent = () => {
    switch (menuKey) {
      case NavType.KNOWLEDGE_TERM:
        return <Terminology key={menuKey} promptType={menuKey} />;
      case NavType.BUSINESS_LOGIC:
        return <Terminology key={menuKey} promptType={menuKey} />;
      case NavType.SQL_TEMPLATE:
        return <Terminology key={menuKey} promptType={menuKey} />;
      case NavType.ANNOTATION_TABLE:
        return <AnnotationTable key={menuKey} />;
      default:
        return null;
    }
  };
  return <div className={cx(styles.container, className)}>{switchContent()}</div>;
});
