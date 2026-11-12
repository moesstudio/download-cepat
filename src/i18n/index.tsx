import React, { createContext, useContext } from 'react';
import {
  LanguageCode,
  TranslationKey,
  translate,
} from './translations.ts';

export type { LanguageCode, LanguageOption, TranslationKey } from './translations.ts';
export { LANGUAGE_OPTIONS, isValidLanguage, translate } from './translations.ts';

export interface I18nContextValue {
  lang: LanguageCode;
  setLang: (next: LanguageCode) => void;
  t: (key: TranslationKey, params?: Record<string, string | number>) => string;
}

const DEFAULT_I18N: I18nContextValue = {
  lang: 'id',
  setLang: () => {},
  t: (key, params) => translate('id', key, params),
};

const I18nContext = createContext<I18nContextValue>(DEFAULT_I18N);

export interface I18nProviderProps {
  lang: LanguageCode;
  setLang: (next: LanguageCode) => void;
  children: React.ReactNode;
}

export const I18nProvider: React.FC<I18nProviderProps> = ({ lang, setLang, children }) => {
  const t = React.useCallback(
    (key: TranslationKey, params?: Record<string, string | number>) => translate(lang, key, params),
    [lang]
  );

  const value = React.useMemo<I18nContextValue>(
    () => ({
      lang,
      setLang,
      t,
    }),
    [lang, setLang, t]
  );

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
};

export const useI18n = (): I18nContextValue => useContext(I18nContext);
