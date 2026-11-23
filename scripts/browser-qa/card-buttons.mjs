// Choosing the primary button of a Ferryx approval card from tesseract's words.
//
// A card reads as a title line over a row of buttons:
//
//   Allow "ferryxqa" to control the browser?      <- title, starts with the label "Allow"
//   A local program asks to pair as a browser ...
//                                    Deny  Allow  <- the button row
//
// The CDP audit clicked the *title's* first word on Windows (run 45024452034: the click landed at
// (581,167), inside the title's "Allow", while the real button's pill is centred at (931,241)),
// because "Windows OCR drops the dark Deny button" made it fall back to the topmost word matching
// the label, and the title is above the buttons. The card then expired unapproved. The same shape
// occurs wherever a title opens with its primary verb ("Allow ...", "Approve ..."), so the choice
// is made here on plain word boxes, apart from the audit, and unit-tested.

/** Buttons that sit left of a card's primary on the same row: their presence identifies the row. */
export const CARD_SIBLINGS = ["Deny", "Cancel", "Done", "Block", "Keep"];

const isWord = (word) =>
  Boolean(word) && typeof word.text === "string" && Number.isFinite(word.left) && Number.isFinite(word.top);

/**
 * The primary button of an approval card, or null when no word is provably a button.
 *
 * `words` are tesseract word boxes ({text, left, top, width, height}); `label` is the primary's
 * exact text ("Allow", "Approve"). `titleTop` is the top of the card's title line when the caller
 * could read it: every word on that line is card text, never a button.
 *
 * A label word is accepted as the button only on positive evidence, in two strictly ranked tiers:
 *  1. (best) a sibling button sits immediately left of it on its row ("Deny" left of "Allow"), or
 *  2. the caller read the title line and this word sits strictly below that line's band with
 *     nothing following it on its row.
 * Anything else is rejected. In particular a lone label word with no sibling and no known title
 * line proves nothing: it is as likely to be a partly-read title (Windows OCR read just the
 * title's "Allow" at (564,152) on run 45024452034), so no click is offered and the caller's pixel
 * fallback runs instead of clicking card text.
 *
 * The ranking matters as much as the evidence. A sibling-anchored button always beats a lone
 * word, and within a tier the *bottom-most* row wins: action buttons sit at the bottom of the
 * card, so a wrapped title word (higher up, smaller `top`) must never outrank the real row. Both
 * rules are what the old "topmost match" lacked.
 */
export function cardButtonPoint(words, label, { titleTop = null, siblings = CARD_SIBLINGS, lineTolerance = 14, wordGap = 40 } = {}) {
  const list = (words ?? []).filter(isWord);
  const knownTitle = Number.isFinite(titleTop);
  const sameRow = (a, b) => Math.abs(a.top - b.top) <= lineTolerance;
  const inRow = (word) => list.filter((other) => other !== word && sameRow(other, word)).sort((a, b) => a.left - b.left);
  const centre = (word) => [
    word.left + Math.round((Number(word.width) || 0) / 2),
    word.top + Math.round((Number(word.height) || 12) / 2),
  ];

  const examined = list
    .filter((word) => word.text === label)
    .map((word) => {
      const row = inRow(word);
      const followed = row.find((other) => other.left > word.left && other.left - (word.left + (Number(word.width) || 0)) < wordGap) ?? null;
      const before = [...row].reverse().find((other) => other.left < word.left) ?? null;
      const sibling = before && siblings.includes(before.text) ? before : null;
      const onTitle = knownTitle && Math.abs(word.top - titleTop) <= lineTolerance;
      const belowTitle = knownTitle && word.top > titleTop + lineTolerance;
      // Tier 2 is the strongest evidence a card can offer (a button row identified by its
      // sibling), tier 1 needs the title line read to prove the word is below it, tier 0 is no
      // evidence at all and is never clicked.
      const tier = sibling ? 2 : belowTitle && !followed ? 1 : 0;
      const why = sibling
        ? `right of ${sibling.text}`
        : onTitle
          ? "on the card title line"
          : followed
            ? `followed by '${followed.text}'`
            : belowTitle
              ? "alone on its row below the title line"
              : "alone on its row with no title line read";
      return { word, tier, why, at: centre(word) };
    });

  const chosen = examined
    .filter((entry) => entry.tier > 0)
    .sort((a, b) => b.tier - a.tier || b.word.top - a.word.top || b.word.left - a.word.left)[0] ?? null;
  const rejected = examined.filter((entry) => entry !== chosen).map((entry) => ({ at: entry.at, why: entry.why }));
  if (!chosen) return { at: null, via: "none", rejected };
  const via = chosen.tier === 2 ? `ocr (${chosen.why})` : "ocr (label alone on its row)";
  return { at: chosen.at, via, rejected };
}
