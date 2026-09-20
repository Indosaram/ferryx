// Hangul composition for mobile IMEs that never compose.
//
// WKWebView's Korean keyboard (iOS/iPadOS Safari and standalone PWAs, Android WebKit) can hand over
// bare compatibility jamo - ㅇ, ㅣ, ㄹ, ㅓ ... - with no composition events at all, and some Android
// keyboards commit one jamo per composition. A keystroke stream the user typed as 이렇게 therefore
// reaches the terminal as ㅇㅣㄹㅓㅎㄱㅔ. The client has to reassemble the syllables itself, exactly
// as the OS keyboard would have.
//
// The rules are the two-set (두벌식) ones, including the final-consonant migrations the OS keyboard
// performs: ㄱㅏㅅㅏ is 가사 (갓 + ㅏ carries the final consonant into the next syllable) and ㄷㅏㄹㄱㅏ
// is 달가 (ㄺ splits). Runs that never form a syllable are left alone - a lone ㅋ, ㄱㄴ, or ㅠㅠ is
// text the user meant, not an unfinished syllable - and a standalone vowel never gains an implicit ㅇ.

/// Initial consonants (초성), index 0..18.
const INITIAL_JAMO = "ㄱㄲㄴㄷㄸㄹㅁㅂㅃㅅㅆㅇㅈㅉㅊㅋㅌㅍㅎ";

/// Medial vowels (중성), index 0..20.
const MEDIAL_JAMO = "ㅏㅐㅑㅒㅓㅔㅕㅖㅗㅘㅙㅚㅛㅜㅝㅞㅟㅠㅡㅢㅣ";

/// Consonant jamo of the compatibility block (U+3131 ㄱ .. U+314E ㅎ), in code-point order.
const CONSONANT_JAMO = "ㄱㄲㄳㄴㄵㄶㄷㄸㄹㄺㄻㄼㄽㄾㄿㅀㅁㅂㅃㅄㅅㅆㅇㅈㅉㅊㅋㅌㅍㅎ";

/// Final consonants (종성), index 0..27; index 0 is "no final consonant" and the tense consonants
/// ㄸ/ㅃ/ㅉ never close a syllable.
const FINAL_JAMO = `\u0000${CONSONANT_JAMO.replace(/[ㄸㅃㅉ]/g, "")}`;

/// The compatibility jamo block (U+3131 ㄱ .. U+3163 ㅣ), in code-point order.
const COMPATIBILITY_JAMO = `${CONSONANT_JAMO}${MEDIAL_JAMO}`;

/// Vowels the two-set keyboard builds from two keystrokes (ㅗ + ㅏ = ㅘ).
const COMPOUND_MEDIALS: Readonly<Record<string, string>> = {
  "ㅗㅏ": "ㅘ",
  "ㅗㅐ": "ㅙ",
  "ㅗㅣ": "ㅚ",
  "ㅜㅓ": "ㅝ",
  "ㅜㅔ": "ㅞ",
  "ㅜㅣ": "ㅟ",
  "ㅡㅣ": "ㅢ",
};

/// Final consonants the two-set keyboard builds from two keystrokes (ㅂ + ㅅ = ㅄ).
const COMPOUND_FINALS: Readonly<Record<string, string>> = {
  "ㄱㅅ": "ㄳ",
  "ㄴㅈ": "ㄵ",
  "ㄴㅎ": "ㄶ",
  "ㄹㄱ": "ㄺ",
  "ㄹㅁ": "ㄻ",
  "ㄹㅂ": "ㄼ",
  "ㄹㅅ": "ㄽ",
  "ㄹㅌ": "ㄾ",
  "ㄹㅍ": "ㄿ",
  "ㄹㅎ": "ㅀ",
  "ㅂㅅ": "ㅄ",
};

/// How a compound final splits when a vowel pulls it into the next syllable: the first part stays as
/// the final consonant of the syllable being closed (닭 + ㅏ → 달가 keeps ㄹ).
const FINAL_SPLIT: Readonly<Record<string, readonly [string, string]>> = {
  "ㄳ": ["ㄱ", "ㅅ"],
  "ㄵ": ["ㄴ", "ㅈ"],
  "ㄶ": ["ㄴ", "ㅎ"],
  "ㄺ": ["ㄹ", "ㄱ"],
  "ㄻ": ["ㄹ", "ㅁ"],
  "ㄼ": ["ㄹ", "ㅂ"],
  "ㄽ": ["ㄹ", "ㅅ"],
  "ㄾ": ["ㄹ", "ㅌ"],
  "ㄿ": ["ㄹ", "ㅍ"],
  "ㅀ": ["ㄹ", "ㅎ"],
  "ㅄ": ["ㅂ", "ㅅ"],
};

type Jamo = { readonly kind: "vowel" | "consonant"; readonly compat: string };

/// Classify one character as a vowel or consonant jamo, normalized to its compatibility form.
/// Returns null for anything that is not a jamo (including precomposed syllables, which are settled
/// text and must never be decomposed).
function classifyJamo(char: string): Jamo | null {
  const code = char.codePointAt(0) ?? 0;
  if (code >= 0x3131 && code <= 0x3163) {
    const compat = COMPATIBILITY_JAMO[code - 0x3131] as string | undefined;
    if (compat === undefined) return null;
    if (MEDIAL_JAMO.includes(compat)) return { kind: "vowel", compat };
    return { kind: "consonant", compat };
  }
  // Conjoining jamo (U+1100 leading, U+1161 medial, U+11A8 trailing) are the decomposition forms an
  // IME can also deliver; they follow the same order as the tables above.
  if (code >= 0x1100 && code <= 0x1112) return { kind: "consonant", compat: INITIAL_JAMO[code - 0x1100] as string };
  if (code >= 0x1161 && code <= 0x1175) return { kind: "vowel", compat: MEDIAL_JAMO[code - 0x1161] as string };
  if (code >= 0x11a8 && code <= 0x11c2) return { kind: "consonant", compat: FINAL_JAMO[code - 0x11a8 + 1] as string };
  return null;
}

/// True when every character is a jamo the client still has to compose. A commit made only of these
/// is an unfinished run, not text the IME has settled.
export function isUncomposedJamoRun(text: string): boolean {
  if (text.length === 0) return false;
  for (const char of text) {
    if (classifyJamo(char) === null) return false;
  }
  return true;
}

/// Fold every run of bare jamo into the syllables a Korean keyboard would have produced, leaving
/// precomposed syllables and all other text untouched. Idempotent on already-composed text.
export function composeJamoRuns(text: string): string {
  let composed = "";
  let initial: number | null = null;
  let medial: number | null = null;
  let final: number | null = null;

  const flush = () => {
    if (initial !== null && medial !== null) {
      composed += String.fromCharCode(0xac00 + (initial * 21 + medial) * 28 + (final ?? 0));
    } else if (initial !== null) {
      composed += INITIAL_JAMO[initial];
    } else if (medial !== null) {
      composed += MEDIAL_JAMO[medial];
    }
    initial = null;
    medial = null;
    final = null;
  };

  for (const char of text) {
    const jamo = classifyJamo(char);
    if (jamo === null) {
      flush();
      composed += char;
      continue;
    }

    if (jamo.kind === "vowel") {
      const vowel = jamo.compat;
      if (initial === null) {
        // A vowel with nothing to attach to stays a vowel: the keyboard never typed an ㅇ.
        flush();
        composed += vowel;
        continue;
      }
      if (medial === null) {
        medial = MEDIAL_JAMO.indexOf(vowel);
        continue;
      }
      const compound = final === null ? COMPOUND_MEDIALS[MEDIAL_JAMO[medial] + vowel] : undefined;
      if (compound !== undefined) {
        medial = MEDIAL_JAMO.indexOf(compound);
        continue;
      }
      // The vowel opens the syllable after this one, taking the final consonant along (갓 + ㅏ → 가사)
      // or the second half of a compound final (닭 + ㅏ → 달가).
      const finalJamo: string = final === null ? "" : String(FINAL_JAMO[final]);
      const split: readonly [string, string] | undefined = FINAL_SPLIT[finalJamo];
      const carried: string = split ? split[1] : finalJamo;
      final = split ? FINAL_JAMO.indexOf(split[0]) : null;
      flush();
      initial = INITIAL_JAMO.indexOf(carried);
      medial = MEDIAL_JAMO.indexOf(vowel);
      continue;
    }

    const consonant = jamo.compat;
    const initialIndex = INITIAL_JAMO.indexOf(consonant);
    if (medial === null) {
      // A consonant cannot follow an initial without a vowel: ㄱㄴ is two lone consonants.
      flush();
      if (initialIndex < 0) composed += consonant;
      else initial = initialIndex;
      continue;
    }
    if (final === null) {
      const finalIndex = FINAL_JAMO.indexOf(consonant);
      if (finalIndex > 0) {
        final = finalIndex;
        continue;
      }
      // ㄸ/ㅃ/ㅉ never close a syllable; they open the next one.
      flush();
      initial = initialIndex;
      continue;
    }
    const compoundFinal: string | undefined = COMPOUND_FINALS[String(FINAL_JAMO[final]) + consonant];
    if (compoundFinal !== undefined) {
      final = FINAL_JAMO.indexOf(compoundFinal);
      continue;
    }
    flush();
    initial = initialIndex;
  }

  flush();
  return composed;
}
