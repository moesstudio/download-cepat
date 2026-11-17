import { en, TranslationKey } from './en.ts';
import { id, ms, ja } from './locales/regional.ts';
import { zh, ko, hi, vi, th } from './locales/asian.ts';
import { es, fr, de, pt, ru, ar, tr } from './locales/westernRtl.ts';

export type LanguageCode =
  | 'en'
  | 'id'
  | 'ms'
  | 'zh'
  | 'ja'
  | 'ko'
  | 'es'
  | 'fr'
  | 'de'
  | 'pt'
  | 'ru'
  | 'ar'
  | 'hi'
  | 'vi'
  | 'th'
  | 'tr';

export interface LanguageOption {
  code: LanguageCode;
  name: string;
  nativeName: string;
  shortLabel: string;
  dir: 'ltr' | 'rtl';
}

export const LANGUAGE_OPTIONS: LanguageOption[] = [
  { code: 'en', name: 'English', nativeName: 'English', shortLabel: 'EN', dir: 'ltr' },
  { code: 'id', name: 'Indonesian', nativeName: 'Bahasa Indonesia', shortLabel: 'ID', dir: 'ltr' },
  { code: 'ms', name: 'Malay', nativeName: 'Bahasa Melayu', shortLabel: 'MS', dir: 'ltr' },
  { code: 'zh', name: 'Chinese (Simplified)', nativeName: '简体中文', shortLabel: 'ZH', dir: 'ltr' },
  { code: 'ja', name: 'Japanese', nativeName: '日本語', shortLabel: 'JA', dir: 'ltr' },
  { code: 'ko', name: 'Korean', nativeName: '한국어', shortLabel: 'KO', dir: 'ltr' },
  { code: 'es', name: 'Spanish', nativeName: 'Español', shortLabel: 'ES', dir: 'ltr' },
  { code: 'fr', name: 'French', nativeName: 'Français', shortLabel: 'FR', dir: 'ltr' },
  { code: 'de', name: 'German', nativeName: 'Deutsch', shortLabel: 'DE', dir: 'ltr' },
  { code: 'pt', name: 'Portuguese', nativeName: 'Português', shortLabel: 'PT', dir: 'ltr' },
  { code: 'ru', name: 'Russian', nativeName: 'Русский', shortLabel: 'RU', dir: 'ltr' },
  { code: 'ar', name: 'Arabic', nativeName: 'العربية', shortLabel: 'AR', dir: 'rtl' },
  { code: 'hi', name: 'Hindi', nativeName: 'हिन्दी', shortLabel: 'HI', dir: 'ltr' },
  { code: 'vi', name: 'Vietnamese', nativeName: 'Tiếng Việt', shortLabel: 'VI', dir: 'ltr' },
  { code: 'th', name: 'Thai', nativeName: 'ไทย', shortLabel: 'TH', dir: 'ltr' },
  { code: 'tr', name: 'Turkish', nativeName: 'Türkçe', shortLabel: 'TR', dir: 'ltr' },
];

const VALID_CODES = new Set<string>(LANGUAGE_OPTIONS.map((o) => o.code));

export const isValidLanguage = (value: string | null | undefined): value is LanguageCode => {
  return typeof value === 'string' && VALID_CODES.has(value);
};

export const isRtlLanguage = (lang: LanguageCode): boolean => lang === 'ar';

export type { TranslationKey };

export const TRANSLATIONS: Record<LanguageCode, Record<TranslationKey, string>> = {
  en,
  id,
  ms,
  zh,
  ja,
  ko,
  es,
  fr,
  de,
  pt,
  ru,
  ar,
  hi,
  vi,
  th,
  tr,
};

export function translate(
  lang: LanguageCode,
  key: TranslationKey,
  params?: Record<string, string | number>
): string {
  const dict = TRANSLATIONS[lang] ?? TRANSLATIONS.en;
  let text: string = dict[key] ?? TRANSLATIONS.en[key] ?? key;
  if (params) {
    for (const [paramKey, paramValue] of Object.entries(params)) {
      text = text.replace(new RegExp(`\\{${paramKey}\\}`, 'g'), String(paramValue));
    }
  }
  return text;
}
