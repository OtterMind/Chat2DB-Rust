import { useGlobalStore } from '@/store/global';
import {getLanguageType} from '@/utils';

export const modifiedGlobalVariable = ()=> {
  modifiedLanguage();
}

// Correct language
const modifiedLanguage = () => {
  const preConversion = useGlobalStore.getState().baseSetting.language;
  const postConversion = getLanguageType(preConversion);
  if (preConversion !== postConversion) {
    useGlobalStore.getState().setLanguage(postConversion);
  }
}

