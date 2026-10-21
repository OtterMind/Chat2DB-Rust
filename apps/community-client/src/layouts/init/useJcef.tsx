import { useEffect } from 'react';
import { useGlobalStore } from '@/store/global';
import jcefApi from '@/jcef';
import { isDesktop } from '@/utils/env';

const useJcef = () => {
  const { appearance, language, enableMcp } = useGlobalStore((state) => {
    return {
      appearance: state.baseSetting.appearance,
      language: state.baseSetting.language,
      enableMcp: state.baseSetting.enableMcp,
    };
  });

  useEffect(() => {
    if (!isDesktop) {
      return;
    }
    jcefApi.updateSettings({
      appearance,
      language,
      enableMcp,
    });
  }, [appearance, language, enableMcp]);
};

export default useJcef;
