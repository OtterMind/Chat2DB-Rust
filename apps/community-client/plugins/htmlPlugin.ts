import { IApi } from 'umi';

export default (api: IApi) => {
  api.modifyHTML(($) => {
    $('script').attr('th:inline', 'none');
    return $;
  });
};
