import { DatabaseTypeCode } from '../common';
import oracle from './oracle';

export default {
  ...oracle,
  type: DatabaseTypeCode.OSCAR,
};
