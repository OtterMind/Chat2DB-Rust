import { useEffect } from 'react';
import { useGlobalStore } from '@/store/global';

const initAppConfig = () => {

  const { queryAppConfig } = useGlobalStore((state) => ({
    queryAppConfig: state.queryAppConfig,
  }));
  queryAppConfig();

  // Get APPConfig
  useEffect(() => {
  }, []); 
};

export default initAppConfig;
