import { defineConfig } from 'umi';
import { extractYarnConfig } from './src/utils/package';
const MonacoWebpackPlugin = require('monaco-editor-webpack-plugin');
const yarn_config = extractYarnConfig(process.argv);
const app_version = yarn_config.app_version || '5.3.0';
const publicPath = yarn_config.public_path || './static/front/';

const chainWebpack = (config: any, { webpack }: any) => {
  config.plugin('monaco-editor').use(MonacoWebpackPlugin, [
    {
      languages: ['mysql', 'pgsql', 'sql'],
    },
  ]);
};
/**


<!-- Google tag (gtag.js) -->
<script async src="https://www.googletagmanager.com/gtag/js?id=G-PLPZ9PBJEY"></script>
<script>
  window.dataLayer = window.dataLayer || [];
  function gtag(){dataLayer.push(arguments);}
  gtag('js', new Date());

  gtag('config', 'G-PLPZ9PBJEY');
</script>

 */
export default defineConfig({
  publicPath,
  chainWebpack,
  headScripts: [
    `window.dataLayer = window.dataLayer || [];
    function gtag() {
      window.dataLayer.push(arguments);
    }
    gtag('js', new Date());
    gtag('config', 'G-PLPZ9PBJEY', {
      platform: 'WEB',
      version: '${app_version}'
    });`,
  ],
});
