import { ChatSourceType } from '@/constants/chat';
import MagicStickService from '@/service/magicStick';

export interface IParams {
  message: string;
  tableList?: any[];
}
const text2Chart = (params: IParams) => {
  return new Promise((resolve, reject) => {
    MagicStickService.text2Chart({
      ...params,
      source: ChatSourceType.SINGLE_TURN_CHAT,
    })
      .then((res: any) => {
        resolve(res);
      })
      .catch((error) => {
        reject(error);
      });
  });
};

export default text2Chart;
