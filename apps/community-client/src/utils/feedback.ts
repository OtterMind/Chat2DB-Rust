import { message } from 'antd';
import { staticMessage } from '@chat2db/ui';
import type { Key } from 'react';
import type { ArgsProps, JointContent, MessageInstance, MessageType } from 'antd/es/message/interface';

type MessageMethod = 'success' | 'error' | 'warning' | 'info' | 'loading';

const getMessageApi = (): MessageInstance => staticMessage || message;

const callMessage = (
  type: MessageMethod,
  content: JointContent,
  duration?: number | VoidFunction,
  onClose?: VoidFunction,
): MessageType => {
  return getMessageApi()[type](content, duration, onClose);
};

const feedback = {
  success(content: JointContent, duration?: number | VoidFunction, onClose?: VoidFunction) {
    return callMessage('success', content, duration, onClose);
  },
  error(content: JointContent, duration?: number | VoidFunction, onClose?: VoidFunction) {
    return callMessage('error', content, duration, onClose);
  },
  warning(content: JointContent, duration?: number | VoidFunction, onClose?: VoidFunction) {
    return callMessage('warning', content, duration, onClose);
  },
  info(content: JointContent, duration?: number | VoidFunction, onClose?: VoidFunction) {
    return callMessage('info', content, duration, onClose);
  },
  loading(content: JointContent, duration?: number | VoidFunction, onClose?: VoidFunction) {
    return callMessage('loading', content, duration, onClose);
  },
  open(args: ArgsProps) {
    return getMessageApi().open(args);
  },
  destroy(key?: Key) {
    return getMessageApi().destroy(key);
  },
};

export default feedback;
