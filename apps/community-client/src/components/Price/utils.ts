import i18n from '@/i18n';
import { LocalizedFeatureMap, ProductDetail, ProductDetailVO } from '@/typings/pricing';
import { SubscriptionType } from '@/constants/pricing';

type ProductDescriptionSource = Partial<
  Pick<
    ProductDetail | ProductDetailVO,
    'shortDescription' | 'description' | 'description1' | 'description2' | 'descriptionList' | 'localizedFeatures'
  >
>;

type ProductAttach = {
  tag?: string;
  description1?: string;
  description2?: string;
};

type ProductDescriptionPayload = {
  feature?: LocalizedFeatureMap;
};

const parseJsonObject = <T>(value?: string) => {
  if (!value) {
    return undefined;
  }

  try {
    return JSON.parse(value) as T;
  } catch {
    return undefined;
  }
};

const parseFeatureMap = (feature?: LocalizedFeatureMap | string) => {
  if (!feature) {
    return undefined;
  }

  if (typeof feature === 'string') {
    const parsedFeature = parseJsonObject<LocalizedFeatureMap | ProductDescriptionPayload>(feature);
    if (parsedFeature && 'feature' in parsedFeature) {
      return parsedFeature.feature;
    }
    return parsedFeature as LocalizedFeatureMap | undefined;
  }

  return feature;
};

export const resolveLocalizedFeatures = (
  feature: LocalizedFeatureMap | string | undefined,
  language?: string,
) => {
  const featureMap = parseFeatureMap(feature);
  if (!featureMap) {
    return [];
  }

  const normalizedLanguage = language || 'en-US';
  const exactMatch = featureMap[normalizedLanguage];
  if (exactMatch?.length) {
    return exactMatch;
  }

  const languagePrefix = normalizedLanguage.split('-')[0];
  const prefixMatchKey = Object.keys(featureMap).find((key) => key.split('-')[0] === languagePrefix);
  if (prefixMatchKey && featureMap[prefixMatchKey]?.length) {
    return featureMap[prefixMatchKey];
  }

  if (featureMap['en-US']?.length) {
    return featureMap['en-US'];
  }

  const firstKey = Object.keys(featureMap)[0];
  return firstKey ? featureMap[firstKey] || [] : [];
};

export const collectProductDescriptions = (product?: ProductDescriptionSource) => {
  if (!product) {
    return [];
  }

  if (product.localizedFeatures?.length) {
    return Array.from(
      new Set(
        product.localizedFeatures
          .map((item) => item?.trim())
          .filter((item): item is string => Boolean(item)),
      ),
    );
  }

  const items = [
    product.description,
    product.description1,
    product.description2,
    ...(product.descriptionList || []),
  ];

  return Array.from(
    new Set(
      items
        .map((item) => item?.trim())
        .filter((item): item is string => Boolean(item)),
    ),
  );
};

export const getSubscriptionTypeLabel = (subscriptionType?: string) => {
  switch (subscriptionType) {
    case SubscriptionType.monthly:
      return i18n('price.text.monthly');
    case SubscriptionType.yearly:
      return i18n('price.text.yearly');
    case SubscriptionType.forever:
      return i18n('price.text.forever');
    default:
      return subscriptionType || '';
  }
};

export const normalizeProductDetail = <T extends ProductDetail | ProductDetailVO>(product: T, language?: string) => {
  const parsedAttach = parseJsonObject<ProductAttach>(product.attach);
  const parsedDescription = parseJsonObject<ProductDescriptionPayload>(product.description);
  const feature = product.feature || parsedDescription?.feature;
  const localizedFeatures = resolveLocalizedFeatures(feature, language);
  const descriptionLooksLikeJson = typeof product.description === 'string' && product.description.trim().startsWith('{');

  return {
    ...product,
    tag: parsedAttach?.tag || product.tag,
    feature,
    localizedFeatures,
    shortDescription: product.shortDescription || parsedAttach?.description1 || product.description1,
    description: descriptionLooksLikeJson ? '' : product.description,
    description1: parsedAttach?.description1 || product.description1,
    description2: parsedAttach?.description2 || product.description2,
  };
};
