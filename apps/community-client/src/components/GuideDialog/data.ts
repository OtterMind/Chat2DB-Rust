import i18n from '@/i18n';
import { GuideDialogStatus } from './type';

interface Feature {
  icon: string;
  title: string;
  subtitle: string;
}

interface Button {
  title: string;
  action?: () => void;
}

interface DialogContent {
  title: string;
  subTitle: string;
  highlightSubTitle: string;
  dividingLine: string;
  feature: Feature[];
  primaryButton: Button;
  primaryButton2?: Button;
  secondaryButton: Button;
}

export const GuideDialogContent: Record<GuideDialogStatus, DialogContent> = {
  [GuideDialogStatus.FirstLogin]: {
    title: i18n('userguide.firstLogin.title'),
    subTitle: i18n('userguide.firstLogin.subTitle'),
    highlightSubTitle: i18n('userguide.firstLogin.highlightSubTitle'),
    dividingLine: i18n('userguide.firstLogin.dividingLine'),
    feature: [
      {
        icon: 'icon-logout1',
        title: i18n('userguide.firstLogin.feature1.title'),
        subtitle: i18n('userguide.firstLogin.feature1.subtitle'),
      },
      {
        icon: 'icon-sparkles',
        title: i18n('userguide.firstLogin.feature2.title'),
        subtitle: i18n('userguide.firstLogin.feature2.subtitle'),
      },
      {
        icon: 'icon-chart-square-bar',
        title: i18n('userguide.firstLogin.feature3.title'),
        subtitle: i18n('userguide.firstLogin.feature3.subtitle'),
      },
      {
        icon: 'icon-xingzhuang',
        title: i18n('userguide.firstLogin.feature4.title'),
        subtitle: i18n('userguide.firstLogin.feature4.subtitle'),
      },
      {
        icon: 'icon-chat-alt-21',
        title: i18n('userguide.firstLogin.feature5.title'),
        subtitle: i18n('userguide.firstLogin.feature5.subtitle'),
      },
    ],
    primaryButton: {
      title: i18n('userguide.firstLogin.primaryButton.title'),
      // action: () => {},
    },
    secondaryButton: {
      title: i18n('userguide.firstLogin.secondaryButton.title'),
      // action: () => {},
    },
  },
  [GuideDialogStatus.Subscribed]: {
    title: i18n('userguide.subscribed.title'),
    subTitle: i18n('userguide.subscribed.subTitle'),
    highlightSubTitle: i18n('userguide.subscribed.highlightSubTitle'),
    dividingLine: i18n('userguide.subscribed.dividingLine'),
    feature: [
      {
        icon: 'icon-logout1',
        title: i18n('userguide.subscribed.feature1.title'),
        subtitle: i18n('userguide.subscribed.feature1.subtitle'),
      },
      {
        icon: 'icon-sparkles',
        title: i18n('userguide.subscribed.feature2.title'),
        subtitle: i18n('userguide.subscribed.feature2.subtitle'),
      },
      {
        icon: 'icon-chart-square-bar',
        title: i18n('userguide.subscribed.feature3.title'),
        subtitle: i18n('userguide.subscribed.feature3.subtitle'),
      },
      {
        icon: 'icon-xingzhuang',
        title: i18n('userguide.subscribed.feature4.title'),
        subtitle: i18n('userguide.subscribed.feature4.subtitle'),
      },
      {
        icon: 'icon-chat-alt-21',
        title: i18n('userguide.subscribed.feature5.title'),
        subtitle: i18n('userguide.subscribed.feature5.subtitle'),
      },
    ],
    primaryButton: {
      title: i18n('userguide.subscribed.primaryButton.title'),
      // action: () => {},
    },
    secondaryButton: {
      title: i18n('userguide.subscribed.secondaryButton.title'),
      // action: () => {},
    },
  },

  [GuideDialogStatus.Expired]: {
    title: i18n('userguide.expired.title'),
    subTitle: i18n('userguide.expired.subTitle'),
    highlightSubTitle: i18n('userguide.expired.highlightSubTitle'),
    dividingLine: i18n('userguide.expired.dividingLine'),
    feature: [
      {
        icon: 'icon-logout1',
        title: i18n('userguide.expired.feature1.title'),
        subtitle: i18n('userguide.expired.feature1.subtitle'),
      },
      {
        icon: 'icon-sparkles',
        title: i18n('userguide.expired.feature2.title'),
        subtitle: i18n('userguide.expired.feature2.subtitle'),
      },
      {
        icon: 'icon-chart-square-bar',
        title: i18n('userguide.expired.feature3.title'),
        subtitle: i18n('userguide.expired.feature3.subtitle'),
      },
      {
        icon: 'icon-xingzhuang',
        title: i18n('userguide.expired.feature4.title'),
        subtitle: i18n('userguide.expired.feature4.subtitle'),
      },
      {
        icon: 'icon-chat-alt-21',
        title: i18n('userguide.expired.feature5.title'),
        subtitle: i18n('userguide.expired.feature5.subtitle'),
      },
    ],
    primaryButton: {
      title: i18n('userguide.expired.primaryButton.title'),
      // action: () => {
      //   setOpenGuideDialog(false)
      // },
    },
    secondaryButton: {
      title: i18n('userguide.expired.secondaryButton.title'),
      // a·ction: () => {},
    },
  },

  [GuideDialogStatus.OfflineTrial]: {
    title: i18n('userguide.offlineTrial.title'),
    subTitle: i18n('userguide.offlineTrial.subTitle'),
    highlightSubTitle: i18n('userguide.offlineTrial.highlightSubTitle'),
    dividingLine: i18n('userguide.offlineTrial.dividingLine'),
    feature: [
      {
        icon: 'icon-logout1',
        title: i18n('userguide.offlineTrial.feature1.title'),
        subtitle: i18n('userguide.offlineTrial.feature1.subtitle'),
      },
      {
        icon: 'icon-sparkles',
        title: i18n('userguide.offlineTrial.feature2.title'),
        subtitle: i18n('userguide.offlineTrial.feature2.subtitle'),
      },
      {
        icon: 'icon-chart-square-bar',
        title: i18n('userguide.offlineTrial.feature3.title'),
        subtitle: i18n('userguide.offlineTrial.feature3.subtitle'),
      },
      {
        icon: 'icon-xingzhuang',
        title: i18n('userguide.offlineTrial.feature4.title'),
        subtitle: i18n('userguide.offlineTrial.feature4.subtitle'),
      },
      {
        icon: 'icon-chat-alt-21',
        title: i18n('userguide.offlineTrial.feature5.title'),
        subtitle: i18n('userguide.offlineTrial.feature5.subtitle'),
      },
    ],
    primaryButton: {
      title: i18n('userguide.offlineTrial.primaryButton.title'),
      // action: () => {},
    },
    primaryButton2: {
      title: i18n('userguide.offlineTrial.primaryButton2.title'),
      // action: () => {},
    },
    secondaryButton: {
      title: i18n('userguide.offlineTrial.secondaryButton.title'),
      // action: () => {},
    },
  },
  [GuideDialogStatus.OfflineTrialExpired]: {
    title: i18n('userguide.offlineTrialExpired.title'),
    subTitle: i18n('userguide.offlineTrialExpired.subTitle'),
    highlightSubTitle: i18n('userguide.offlineTrialExpired.highlightSubTitle'),
    dividingLine: i18n('userguide.offlineTrialExpired.dividingLine'),
    feature: [
      {
        icon: 'icon-logout1',
        title: i18n('userguide.offlineTrialExpired.feature1.title'),
        subtitle: i18n('userguide.offlineTrialExpired.feature1.subtitle'),
      },
      {
        icon: 'icon-sparkles',
        title: i18n('userguide.offlineTrialExpired.feature2.title'),
        subtitle: i18n('userguide.offlineTrialExpired.feature2.subtitle'),
      },
      {
        icon: 'icon-chart-square-bar',
        title: i18n('userguide.offlineTrialExpired.feature3.title'),
        subtitle: i18n('userguide.offlineTrialExpired.feature3.subtitle'),
      },
      {
        icon: 'icon-xingzhuang',
        title: i18n('userguide.offlineTrial.feature4.title'),
        subtitle: i18n('userguide.offlineTrial.feature4.subtitle'),
      },
      {
        icon: 'icon-chat-alt-21',
        title: i18n('userguide.offlineTrial.feature5.title'),
        subtitle: i18n('userguide.offlineTrial.feature5.subtitle'),
      },
    ],
    primaryButton: {
      title: i18n('userguide.offlineTrialExpired.primaryButton.title'),
      // action: () => {},
    },
    primaryButton2: {
      title: i18n('userguide.offlineTrialExpired.primaryButton2.title'),
      // action: () => {},
    },
    secondaryButton: {
      title: i18n('userguide.offlineTrialExpired.secondaryButton.title'),
      // action: () => {  },
    },
  },
  [GuideDialogStatus.OfflineLicenseExpired]: {
    title: i18n('userguide.offlineLicenseExpired.title'),
    subTitle: i18n('userguide.offlineLicenseExpired.subTitle'),
    highlightSubTitle: i18n('userguide.offlineLicenseExpired.highlightSubTitle'),
    dividingLine: i18n('userguide.offlineLicenseExpired.dividingLine'),
    feature: [
      {
        icon: 'icon-logout1',
        title: i18n('userguide.offlineTrialExpired.feature1.title'),
        subtitle: i18n('userguide.offlineTrialExpired.feature1.subtitle'),
      },
      {
        icon: 'icon-sparkles',
        title: i18n('userguide.offlineTrialExpired.feature2.title'),
        subtitle: i18n('userguide.offlineTrialExpired.feature2.subtitle'),
      },
      {
        icon: 'icon-chart-square-bar',
        title: i18n('userguide.offlineTrialExpired.feature3.title'),
        subtitle: i18n('userguide.offlineTrialExpired.feature3.subtitle'),
      },
      {
        icon: 'icon-xingzhuang',
        title: i18n('userguide.offlineTrial.feature4.title'),
        subtitle: i18n('userguide.offlineTrial.feature4.subtitle'),
      },
      {
        icon: 'icon-chat-alt-21',
        title: i18n('userguide.offlineTrial.feature5.title'),
        subtitle: i18n('userguide.offlineTrial.feature5.subtitle'),
      },
    ],
    primaryButton: {
      title: i18n('userguide.offlineTrialExpired.primaryButton.title'),
      // action: () => {},
    },
    primaryButton2: {
      title: i18n('userguide.offlineTrialExpired.primaryButton2.title'),
      // action: () => {},
    },
    secondaryButton: {
      title: i18n('userguide.offlineTrialExpired.secondaryButton.title'),
      // action: () => {  },
    },
  },
};
