import { notification } from 'antd';

export default () => {
  notification.config({
    placement: 'bottomRight',
    maxCount: 1,
  });
};
