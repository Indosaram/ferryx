export const MASCOT_SEQUENCE_IDS = [
  "01",
  "02",
  "03",
  "04",
  "05",
  "06",
  "07",
  "08",
  "09",
  "10",
  "11",
  "12",
  "13",
  "14",
  "15",
  "16",
  "17",
  "18",
  "19",
  "20",
] as const;

export type MascotSequenceId = (typeof MASCOT_SEQUENCE_IDS)[number];

export type MascotRng = () => number;

export interface MascotSequenceState {
  readonly currentId: MascotSequenceId;
  readonly bag: readonly MascotSequenceId[];
  readonly index: number;
}

export function isMascotSequenceId(id: string): id is MascotSequenceId {
  return (MASCOT_SEQUENCE_IDS as readonly string[]).includes(id);
}

export function createShuffledBag(
  rng: MascotRng = Math.random,
  previousLastId?: MascotSequenceId,
): readonly MascotSequenceId[] {
  const bag: MascotSequenceId[] = [...MASCOT_SEQUENCE_IDS];

  for (let i = bag.length - 1; i > 0; i--) {
    const raw = rng();
    const j = Math.floor(raw * (i + 1));
    const clampedJ = Math.min(i, Math.max(0, j));
    const temp = bag[i];
    bag[i] = bag[clampedJ];
    bag[clampedJ] = temp;
  }

  if (bag.length > 1 && previousLastId !== undefined && bag[0] === previousLastId) {
    const swapTarget = 1;
    const temp = bag[0];
    bag[0] = bag[swapTarget];
    bag[swapTarget] = temp;
  }

  return Object.freeze(bag);
}

export function createMascotSequence(
  rng: MascotRng = Math.random,
  previousLastId?: MascotSequenceId,
): MascotSequenceState {
  const bag = createShuffledBag(rng, previousLastId);
  return Object.freeze({
    currentId: bag[0],
    bag,
    index: 0,
  });
}

export function advanceMascotSequence(
  state: MascotSequenceState,
  rng: MascotRng = Math.random,
): MascotSequenceState {
  const nextIndex = state.index + 1;
  if (nextIndex < state.bag.length) {
    return Object.freeze({
      currentId: state.bag[nextIndex],
      bag: state.bag,
      index: nextIndex,
    });
  }

  const previousLastId = state.bag[state.bag.length - 1];
  const newBag = createShuffledBag(rng, previousLastId);
  return Object.freeze({
    currentId: newBag[0],
    bag: newBag,
    index: 0,
  });
}

export function getCurrentMascotId(state: MascotSequenceState): MascotSequenceId {
  return state.currentId;
}
