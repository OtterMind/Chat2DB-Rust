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

// Calculate the user's timestamp from the timestamp in time zone 0
export function getUserTimezoneTimestamp(timestamp: number | string) {
  const timezoneOffset = new Date().getTimezoneOffset() * 60 * 1000;
  return +timestamp - timezoneOffset;
}

// Given two timestamps, calculate the time difference between the two timestamps and return seconds
export function getTimestampDiff(start: number, end: number) {
  return (end - start) / 1000;
}
