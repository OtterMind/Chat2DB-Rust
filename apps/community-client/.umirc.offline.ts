import { extractYarnConfig } from './src/utils/package';
import { defineConfig } from 'umi';
const MonacoWebpackPlugin = require('monaco-editor-webpack-plugin');

const yarn_config = extractYarnConfig(process.argv);

const chainWebpack = (config: any, { webpack }: any) => {
  config.plugin('monaco-editor').use(MonacoWebpackPlugin, [
    {
      languages: ['mysql', 'pgsql', 'sql', 'json'],
    },
  ]);
};

export default defineConfig({
  title: 'Chat2DB Local',
  history: {
    type: 'hash',
  },
  publicPath: process.env.NODE_ENV === 'production' ? './' : '/',
  chainWebpack,
  headScripts: [
    `window.dataLayer = window.dataLayer || [];
    function gtag() {
      window.dataLayer.push(arguments);
    }
    gtag('js', new Date());
    gtag('config', 'G-PLPZ9PBJEY', {
      platform: 'DESKTOP',
      version: '${yarn_config['app_version']}'
    });`,
  ],
  scripts: [
    `
      var Chat2DB_Global_Store = localStorage.getItem('Chat2DB_Global_Store');
      var theme = ''
      if(Chat2DB_Global_Store){
        const globalStore = JSON.parse(Chat2DB_Global_Store);
        if(globalStore && globalStore.state.baseSetting.appearance && globalStore.state.baseSetting){
          theme = globalStore.state.baseSetting.appearance
        }
      }
      var backgroundColor = theme.includes('dark') ? '#000' : '#fff';
      var color = theme.includes('dark') ? '#fff' : '#000';
      var appPath = './logo-transparent.webp';
      var script = document.createElement('script');
      script.innerHTML = "var style = document.createElement('style'); style.type = 'text/css'; style.innerHTML = '.open_screen_animation_box{width: 280px} #open_screen_animation { position: absolute; top: 0; bottom: 0; left: 0; right: 0; display: flex; align-items: center; justify-content: center; flex-direction: column; background-color:"+backgroundColor+"; color:"+color+"; z-index: 10000;} .open_screen_animation_center {margin-left: -120px; display: flex; align-items: center; justify-content: center;} @keyframes img_animation { 0% { transform: translateX(100px); } 100% { transform: translateX(0px); } } @keyframes text_animation { 0% { transform: translateX(-200px); } 100% { transform: translateX(0px); } } @keyframes disNone { 0% { display: flex; } 100% { display: none; } } .brand_name { display: flex; justify-content: flex-end; overflow: hidden; } .brand_name_text { line-height: 40px; font-size: 45px; font-weight: bold; font-family: sans-serif; width: 200px; height: 40px; text-align: right; transform: translateX(-200px); animation: text_animation 0.888s forwards; } .img_box { display: flex; justify-content: flex-end; border-radius: 10%; height: 80px; width: 200px; position: relative; z-index: 2; background-color: "+backgroundColor+"; transform: translateX(80px); animation: img_animation 0.888s forwards; } .img_box img { display: block; width: 80px; height: 80px; } #open_screen_animation_expand{height: 40px; width: 280px}'; document.getElementsByTagName('head')[0].appendChild(style); var open_screen_animation = document.createElement('div'); open_screen_animation.id = 'open_screen_animation'; open_screen_animation.innerHTML = '<div class=\\"open_screen_animation_box\\"><div class=\\"open_screen_animation_center\\"><div class=\\"img_box\\"><img src="+appPath+" alt=\\"\\" /></div><div class=\\"brand_name\\"><div class=\\"brand_name_text\\">Chat2DB Local</div></div></div></div><div id=\\"open_screen_animation_expand\\"></div>'; document.body.appendChild(open_screen_animation);";
      document.body.appendChild(script);
    `,
  ],
});
