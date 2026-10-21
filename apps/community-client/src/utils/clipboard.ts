import jcefApi from '@/jcef';
import { isDesktop } from '@/utils/env';

export function readClipboard() {
  if (isDesktop) {
    return jcefApi?.readClipboard?.() ?? Promise.resolve('');
  }

  return navigator.clipboard.readText();
}
