// Umi config cannot resolve @/ aliases here, so this module must stay dependency-free.

// Separate the parameters added in the yarn startup command
export function extractYarnConfig(argv: string[]) {
  const newArgv = argv.slice(2);
  const yarn_config: { [k in string]: string } = {};
  newArgv.forEach((t) => {
    if (t && t.startsWith('--')) {
      const regex = /--(.+?)=(.+)/;
      const matches = t.match(regex);
      if (matches) {
        const key = matches[1];
        const value = matches[2];
        yarn_config[key] = value;
      }
    }
  });
  return yarn_config;
}

export function formatDate(date: any, fmt = 'yyyy-MM-dd') {
  if (!date) {
    return '';
  }
  const parsedDate = typeof date === 'number' || typeof date === 'string' ? new Date(date) : date;
  if (!(parsedDate instanceof Date) || Number.isNaN(parsedDate.getTime())) {
    return '';
  }
  const o: any = {
    'M+': parsedDate.getMonth() + 1,
    'd+': parsedDate.getDate(),
    'h+': parsedDate.getHours(),
    'm+': parsedDate.getMinutes(),
    's+': parsedDate.getSeconds(),
    'q+': Math.floor((parsedDate.getMonth() + 3) / 3),
    S: parsedDate.getMilliseconds(),
  };
  let formatted = fmt;
  if (/(y+)/.test(formatted)) {
    formatted = formatted.replace(RegExp.$1, String(parsedDate.getFullYear()).slice(4 - RegExp.$1.length));
  }
  for (const k in o)
    if (new RegExp(`(${k})`).test(formatted)) {
      formatted = formatted.replace(
        RegExp.$1,
        RegExp.$1.length === 1 ? o[k] : `00${o[k]}`.slice(String(o[k]).length),
      );
    }
  return formatted;
}

export function generateBuildTime() {
  // Get current server time
  const serverTime = new Date();

  // Get the time difference (minutes) of the server time zone
  const serverOffset = serverTime.getTimezoneOffset() * 60000;

  // Convert server time to UTC time
  const utcTime = serverTime.getTime() + serverOffset;

  // Time difference of Beijing time (UTC+8)
  const beijingOffset = 8 * 60 * 60000;

  // Convert UTC time to Beijing time
  const beijingTime = new Date(utcTime + beijingOffset);

  // Format time
  return formatDate(beijingTime, 'yyyy-MM-dd hh:mm:ss');
}
