import { message } from 'antd';
import { Platform } from '@/constants/os';

export const createTop = () => {
  switch (window.navigator.os_type) {
    case Platform.Mac:
      return 38;
    case Platform.Windows:
      return 48;
    default:
      return 8;
  }
};

export default () => {
  message.config({
    maxCount: 1,
    duration: 3,
    top: createTop(),
  });
};
