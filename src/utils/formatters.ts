import { LanguageCode, translate } from '../i18n/translations.ts';

export function formatEta(
  seconds?: number | null,
  verbose: boolean = false,
  lang: LanguageCode = 'id'
): string {
  if (!seconds || seconds <= 0 || !Number.isFinite(seconds)) return '-';
  const total = Math.ceil(seconds);
  const hrs = Math.floor(total / 3600);
  const mins = Math.floor((total % 3600) / 60);
  const secs = total % 60;

  const hUnit = translate(lang, 'common.hoursUnit');
  const mUnit = translate(lang, 'common.minutesUnit');
  const sShort = translate(lang, 'common.bytesUnitSec');
  const secWord = translate(lang, 'common.secondsWord');

  if (hrs > 0) {
    return verbose ? `${hrs}${hUnit} ${mins}${mUnit} ${secs}${sShort}` : `${hrs}${hUnit} ${mins}${mUnit}`;
  }
  if (mins > 0) {
    return verbose ? `${mins}${mUnit} ${secs}${sShort}` : `${mins}${mUnit} ${secs}s`;
  }
  return verbose ? `${secs} ${secWord}` : `${secs}s`;
}
