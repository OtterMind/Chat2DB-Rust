import { CSSProperties, HTMLAttributes, ReactNode } from 'react';
import { useStyles } from './style';

interface MainSecondaryPanelProps extends HTMLAttributes<HTMLDivElement> {
  children: ReactNode;
  width?: CSSProperties['width'];
  bordered?: boolean;
}

const MainSecondaryPanel = ({
  children,
  className,
  style,
  width,
  bordered = false,
  ...rest
}: MainSecondaryPanelProps) => {
  const { styles } = useStyles({
    appTitleBarHeight: window._appTitleBarHeight,
    bordered,
  });

  return (
    <div className={[styles.panel, className].filter(Boolean).join(' ')} style={{ width, ...style }} {...rest}>
      {children}
    </div>
  );
};

export default MainSecondaryPanel;
