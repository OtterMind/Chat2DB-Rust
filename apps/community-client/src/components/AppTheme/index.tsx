import { ReactNode, memo, useMemo } from 'react';
import { PrimaryColors, ThemeProvider, ThemeAppearance } from '@chat2db/ui';
import { useGlobalStore } from '@/store/global';
import { settingSelectors } from '@/store/global/selectors';

export interface AppThemeProps {
  children?: ReactNode;
}

const AppTheme = memo<AppThemeProps>(({ children }) => {
  const { primaryColor, appearance, customFont, customFontSize } = useGlobalStore((state) => {
    return {
      ...settingSelectors.currentBaseSetting(state),
    };
  });

  const themeMode = useMemo(() => {
    if (appearance.includes('dark')) {
      return 'dark';
    } else if (appearance.includes('light')) {
      return 'light';
    }
    return 'auto';
  }, [appearance]);

  return (
    <ThemeProvider
      primaryColor={(primaryColor?.label as PrimaryColors) || 'purple'}
      themeMode={themeMode}
      appearance={appearance === ThemeAppearance.Auto ? undefined : appearance}
      defaultAppearance={appearance}
      customBaseToken={{
        fontFamily: customFont || '',
        fontSize: customFontSize,
      }}
    >
      {children}
    </ThemeProvider>
  );
});

export default AppTheme;
