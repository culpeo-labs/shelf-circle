/**
 * BCP-47 code -> readable language name, e.g. "es" -> "Spanish". Display
 * locale is fixed to English for now — app copy is English-only in v1 (see
 * frontend.md §8), so translated language names would be inconsistent with
 * everything else on screen.
 */
export function languageName(code: string): string {
  try {
    return new Intl.DisplayNames(['en'], { type: 'language' }).of(code) ?? code;
  } catch {
    return code;
  }
}
