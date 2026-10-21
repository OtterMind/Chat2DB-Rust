const fs = require('fs');
const packageJsonPath = './package.json';

// Set app version
// Because the name field in package.json cannot be uppercase
const appName = process.argv[2] || 'chat2db-pro';
// const capitalAppName = process.argv[3] || 'Chat2DB-Pro';

const config = {
  'chat2db-pro': {
    name: 'chat2db-pro',
    capitalAppName: 'Chat2DB-Pro',
    iconPath: 'src/assets/logo/pro',
  },
  'chat2db-pro-test': {
    name: 'chat2db-pro-test',
    capitalAppName: 'Chat2DB-Pro-Test',
    iconPath: 'src/assets/logo/test',
  },
  'chat2db-local': {
    name: 'chat2db-local',
    capitalAppName: 'Chat2DB-Local',
    iconPath: 'src/assets/logo/local',
  },
  'chat2db-local-test': {
    name: 'chat2db-local',
    capitalAppName: 'Chat2DB-Local',
    iconPath: 'src/assets/logo/local',
  },
};

const appConfig = config[appName] || config['chat2db-pro'];

// Replace Chat2DB-Pro in Chat2DB-Pro-${version}-${arch}.${ext} with the specified appName
function replaceArtifactName(name, appName) {
  return name.replace(/Chat2DB-Pro/g, appName);
}

// Read package.json file
fs.readFile(packageJsonPath, 'utf8', (err, data) => {
  if (err) {
    console.error('Error reading package.json:', err);
    return;
  }

  // Parse package.json into objects
  const packageJson = JSON.parse(data);

  packageJson.name = appName;

  packageJson.build.protocols = [
    {
      name: appName,
      schemes: [appName],
    },
  ];

  packageJson.build.mac.artifactName = replaceArtifactName(
    packageJson.build.mac.artifactName,
    appConfig.capitalAppName,
  );
  packageJson.build.win.artifactName = replaceArtifactName(
    packageJson.build.win.artifactName,
    appConfig.capitalAppName,
  );
  packageJson.build.linux.artifactName = replaceArtifactName(
    packageJson.build.linux.artifactName,
    appConfig.capitalAppName,
  );

  packageJson.build.mac.icon = `${appConfig.iconPath}/logo.icns`;
  packageJson.build.win.icon = `${appConfig.iconPath}/logo.ico`;
  packageJson.build.linux.icon = `${appConfig.iconPath}/logo.png`;

  // Write the modified package.json back to the file
  fs.writeFile(packageJsonPath, JSON.stringify(packageJson, null, 2), 'utf8', (err) => {
    if (err) {
      console.error('Error writing package.json:', err);
      return;
    }
    // Next you can proceed with the build operation...
  });
});
