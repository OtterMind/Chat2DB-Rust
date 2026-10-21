import { useLayoutEffect } from 'react';
import { JcefEventBus, JavaPushActionType } from './eventBus';
import jcefApi from '@/jcef';
import { isDesktop } from '@/utils/env';

interface IJavaPushMessage<T> {
  uuid: string;
  actionType: JavaPushActionType;
  message: T;
}

// eg: { uuid: '1234', actionType: 'ai_sse_message', message: { data: 'some data' } }
// eg: { uuid: '12345', actionType: 'update_progress', message: { status: '', progress: '100' } }
// eg: { actionType: 'startup_complete', message: 'CHAT2DB_IPC_RESPONSE_SERVICE_STATUS_SUCCESS' }

function useJavaMessageReceiver() {
    useLayoutEffect(() => {
    if (!isDesktop) {
      return;
    }
    // Define the receiving function and mount it on window
    window.handleJavaMessage = (data) => {
      const obj: IJavaPushMessage<any> = JSON.parse(data);
      const { uuid, actionType, message } = obj;
      if (uuid) {
        JcefEventBus.publish(`${actionType}_${uuid}`, message);
      } else {
        JcefEventBus.publish(actionType, message);
      }
    };
    jcefApi.handleJavaMessageIsReady()

    // Cleanup function
    return () => {
      delete (window as any).handleJavaMessage;
    };
  }, []);

  return null;
}

export default useJavaMessageReceiver;
