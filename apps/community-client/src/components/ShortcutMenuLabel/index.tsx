import React, { useMemo } from 'react';

import {
  ShortcutAction,
  ShortcutOverrides,
  getEffectiveShortcutConfigMap,
  getShortcutLabel,
} from '@/constants/shortcut';
import { useGlobalStore } from '@/store/global';
import { useStyles } from './style';

interface IProps {
  label: React.ReactNode;
  shortcutAction?: ShortcutAction;
}

const ShortcutMenuLabel = (props: IProps) => {
  const { label, shortcutAction } = props;
  const { styles } = useStyles();
  const shortcutOverrides = useGlobalStore((state) => state.shortcutOverrides);

  const shortcutLabel = useMemo(() => {
    if (!shortcutAction) {
      return '';
    }
    const shortcutConfig = getEffectiveShortcutConfigMap(shortcutOverrides as ShortcutOverrides);
    return getShortcutLabel(shortcutConfig[shortcutAction]?.binding);
  }, [shortcutAction, shortcutOverrides]);

  if (!shortcutLabel) {
    return <>{label}</>;
  }

  return (
    <span className={styles.label}>
      <span className={styles.text}>{label}</span>
      <span className={styles.shortcut}>{shortcutLabel}</span>
    </span>
  );
};

export default ShortcutMenuLabel;
