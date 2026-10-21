import dayjs from 'dayjs';

/**
 * Whether you are a founding member
 */
export const isFoundingMember = (curOrgSubscription) => {
  return curOrgSubscription && dayjs(curOrgSubscription?.startTime).isBefore(dayjs('2024-05-08'));
};
