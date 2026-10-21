import { OrgUserRoleCode } from '@/typings/enterprise/organization';

// Is it an administrator?
export const checkIsAdmin = (roleCodes: OrgUserRoleCode[] | null | undefined) => {
  return (
    !!roleCodes?.length &&
    (roleCodes.includes(OrgUserRoleCode.ADMIN) || roleCodes.includes(OrgUserRoleCode.SUPER_ADMIN))
  );
};

// Is it a super administrator?
export const checkIsSuperAdmin = (roleCodes: OrgUserRoleCode[] | null | undefined) => {
  return !!roleCodes?.length && roleCodes.includes(OrgUserRoleCode.SUPER_ADMIN);
};

// isOperator
export const checkIsOperator = (roleCodes: OrgUserRoleCode[] | null | undefined) => {
  return !!roleCodes?.length && roleCodes.includes(OrgUserRoleCode.OPERATOR);
};
