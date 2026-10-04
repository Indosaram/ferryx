import { describe, expect, it } from "vitest";

import {
  advanceMascotSequence,
  createMascotSequence,
  createShuffledBag,
  getCurrentMascotId,
  isMascotSequenceId,
  MASCOT_SEQUENCE_IDS,
  type MascotRng,
  type MascotSequenceId,
} from "./mascotSequence";

function getRngValuesForPermutation(
  target: readonly MascotSequenceId[],
  initial: readonly MascotSequenceId[] = MASCOT_SEQUENCE_IDS,
): number[] {
  const current = [...initial];
  const rngValues: number[] = [];
  for (let i = current.length - 1; i > 0; i--) {
    const targetElem = target[i];
    const j = current.indexOf(targetElem);
    rngValues.push((j + 0.1) / (i + 1));
    const temp = current[i];
    current[i] = current[j];
    current[j] = temp;
  }
  return rngValues;
}

function createLcgRng(seed = 12345): MascotRng {
  let s = seed >>> 0;
  return () => {
    s = (Math.imul(1664525, s) + 1013904223) >>> 0;
    return s / 4294967296;
  };
}

describe("mascotSequence catalog and identifiers", () => {
  it("contains exactly twenty distinct padded identifiers from 01 to 20", () => {
    expect(MASCOT_SEQUENCE_IDS).toHaveLength(20);
    const unique = new Set(MASCOT_SEQUENCE_IDS);
    expect(unique.size).toBe(20);

    const expected = Array.from({ length: 20 }, (_, index) => String(index + 1).padStart(2, "0"));
    expect(MASCOT_SEQUENCE_IDS).toEqual(expected);
  });

  it("validates identifiers with isMascotSequenceId", () => {
    expect(isMascotSequenceId("01")).toBe(true);
    expect(isMascotSequenceId("20")).toBe(true);
    expect(isMascotSequenceId("10")).toBe(true);

    expect(isMascotSequenceId("00")).toBe(false);
    expect(isMascotSequenceId("21")).toBe(false);
    expect(isMascotSequenceId("1")).toBe(false);
    expect(isMascotSequenceId("dance")).toBe(false);
    expect(isMascotSequenceId("")).toBe(false);
  });
});

describe("createShuffledBag", () => {
  it("produces a bag containing all twenty identifiers without duplicates", () => {
    const bag = createShuffledBag();
    expect(bag).toHaveLength(20);
    expect(new Set(bag).size).toBe(20);
    for (const id of MASCOT_SEQUENCE_IDS) {
      expect(bag).toContain(id);
    }
  });

  it("freezes the returned bag array", () => {
    const bag = createShuffledBag();
    expect(Object.isFrozen(bag)).toBe(true);
  });

  it("consumes RNG exactly nineteen times for twenty elements", () => {
    let calls = 0;
    const trackingRng: MascotRng = () => {
      calls++;
      return 0.5;
    };

    createShuffledBag(trackingRng);
    expect(calls).toBe(19);
  });

  it("follows Fisher-Yates shuffle deterministically with injectable RNG", () => {
    const reversedTarget = [...MASCOT_SEQUENCE_IDS].reverse();
    const rngValues = getRngValuesForPermutation(reversedTarget);

    let index = 0;
    const deterministicRng: MascotRng = () => rngValues[index++];

    const bag = createShuffledBag(deterministicRng);
    expect(bag).toEqual(reversedTarget);
  });
});

describe("createMascotSequence", () => {
  it("initializes state at index zero pointing to the first bag element", () => {
    const state = createMascotSequence();
    expect(state.index).toBe(0);
    expect(state.currentId).toBe(state.bag[0]);
    expect(getCurrentMascotId(state)).toBe(state.currentId);
    expect(state.bag).toHaveLength(20);
  });

  it("freezes the returned sequence state", () => {
    const state = createMascotSequence();
    expect(Object.isFrozen(state)).toBe(true);
    expect(Object.isFrozen(state.bag)).toBe(true);
  });

  it("consumes RNG only during bag creation on initialization", () => {
    let calls = 0;
    const trackingRng: MascotRng = () => {
      calls++;
      return 0.25;
    };

    createMascotSequence(trackingRng);
    expect(calls).toBe(19);
  });
});

describe("advanceMascotSequence within-bag transitions", () => {
  it("advances sequentially from index 0 to 19 without consuming RNG", () => {
    let rngCalls = 0;
    const trackingRng: MascotRng = () => {
      rngCalls++;
      return 0.33;
    };

    let state = createMascotSequence(trackingRng);
    expect(rngCalls).toBe(19);

    const initialBag = state.bag;

    for (let step = 1; step < 20; step++) {
      state = advanceMascotSequence(state, trackingRng);
      expect(state.index).toBe(step);
      expect(state.currentId).toBe(initialBag[step]);
      expect(state.bag).toBe(initialBag);
      expect(rngCalls).toBe(19);
    }
  });

  it("preserves input state immutability across calls", () => {
    const initial = createMascotSequence();
    const advanced1 = advanceMascotSequence(initial);
    const advanced2 = advanceMascotSequence(initial);

    expect(initial.index).toBe(0);
    expect(initial.currentId).toBe(initial.bag[0]);

    expect(advanced1.index).toBe(1);
    expect(advanced1.currentId).toBe(initial.bag[1]);

    expect(advanced2.index).toBe(1);
    expect(advanced2.currentId).toBe(initial.bag[1]);

    expect(advanced1).toEqual(advanced2);
  });
});

describe("forty draws and two permutations acceptance requirement", () => {
  it("draws forty times producing two full permutations without adjacent repetition", () => {
    let rngCalls = 0;
    const lcg = createLcgRng(99999);
    const trackingRng: MascotRng = () => {
      rngCalls++;
      return lcg();
    };

    let state = createMascotSequence(trackingRng);
    const draws: MascotSequenceId[] = [state.currentId];

    for (let i = 1; i < 40; i++) {
      state = advanceMascotSequence(state, trackingRng);
      draws.push(state.currentId);
    }

    expect(draws).toHaveLength(40);

    const firstTwenty = draws.slice(0, 20);
    expect(new Set(firstTwenty).size).toBe(20);
    for (const id of MASCOT_SEQUENCE_IDS) {
      expect(firstTwenty).toContain(id);
    }

    const secondTwenty = draws.slice(20, 40);
    expect(new Set(secondTwenty).size).toBe(20);
    for (const id of MASCOT_SEQUENCE_IDS) {
      expect(secondTwenty).toContain(id);
    }

    for (let i = 0; i < draws.length - 1; i++) {
      expect(draws[i]).not.toBe(draws[i + 1]);
    }

    expect(draws[19]).not.toBe(draws[20]);
    expect(rngCalls).toBe(38);
  });
});

describe("boundary swap and forced collision resolution", () => {
  it("swaps index 0 and 1 when the next bag starts with previous bag's final ID", () => {
    const perm1 = [...MASCOT_SEQUENCE_IDS];
    const idx07 = perm1.indexOf("07");
    const temp1 = perm1[19];
    perm1[19] = perm1[idx07];
    perm1[idx07] = temp1;

    const perm2 = [...MASCOT_SEQUENCE_IDS];
    const idx07Second = perm2.indexOf("07");
    const temp2 = perm2[0];
    perm2[0] = perm2[idx07Second];
    perm2[idx07Second] = temp2;

    const idx14Second = perm2.indexOf("14");
    const temp3 = perm2[1];
    perm2[1] = perm2[idx14Second];
    perm2[idx14Second] = temp3;

    const rng1Vals = getRngValuesForPermutation(perm1);
    const rng2Vals = getRngValuesForPermutation(perm2);
    const combinedVals = [...rng1Vals, ...rng2Vals];

    let cursor = 0;
    const collisionRng: MascotRng = () => combinedVals[cursor++];

    let state = createMascotSequence(collisionRng);
    const draws: MascotSequenceId[] = [state.currentId];

    for (let i = 1; i < 40; i++) {
      state = advanceMascotSequence(state, collisionRng);
      draws.push(state.currentId);
    }

    expect(draws[19]).toBe("07");
    expect(draws[20]).not.toBe("07");
    expect(draws[20]).toBe("14");
    expect(draws[21]).toBe("07");

    const secondBag = draws.slice(20, 40);
    expect(new Set(secondBag).size).toBe(20);
    for (const id of MASCOT_SEQUENCE_IDS) {
      expect(secondBag).toContain(id);
    }
  });

  it("applies boundary swap during createShuffledBag when previousLastId matches initial element", () => {
    const target = [...MASCOT_SEQUENCE_IDS];
    const rngVals = getRngValuesForPermutation(target);

    let cursor1 = 0;
    const rng1: MascotRng = () => rngVals[cursor1++];
    const bagWithoutCollision = createShuffledBag(rng1, "99" as MascotSequenceId);
    expect(bagWithoutCollision[0]).toBe("01");
    expect(bagWithoutCollision[1]).toBe("02");

    let cursor2 = 0;
    const rng2: MascotRng = () => rngVals[cursor2++];
    const bagWithCollision = createShuffledBag(rng2, "01");
    expect(bagWithCollision[0]).toBe("02");
    expect(bagWithCollision[1]).toBe("01");
    expect(new Set(bagWithCollision).size).toBe(20);
  });
});

describe("independent instances", () => {
  it("maintains isolated state and RNG consumption across instances", () => {
    const lcg1 = createLcgRng(1001);
    const lcg2 = createLcgRng(2002);

    let stateA = createMascotSequence(lcg1);
    let stateB = createMascotSequence(lcg2);

    const initialAId = stateA.currentId;
    const initialBId = stateB.currentId;

    stateA = advanceMascotSequence(stateA, lcg1);
    stateA = advanceMascotSequence(stateA, lcg1);

    expect(stateA.index).toBe(2);
    expect(stateB.index).toBe(0);
    expect(stateB.currentId).toBe(initialBId);

    stateB = advanceMascotSequence(stateB, lcg2);
    expect(stateB.index).toBe(1);
    expect(stateA.index).toBe(2);
    expect(stateA.currentId).not.toBe(initialAId);
  });
});

describe("long-run sequence invariants across multiple bags", () => {
  it("guarantees every 20-dance block is a complete permutation and no adjacent duplicates occur over 2000 draws", () => {
    const lcg = createLcgRng(54321);
    let state = createMascotSequence(lcg);
    const totalDraws = 2000;
    const allDraws: MascotSequenceId[] = [state.currentId];

    for (let i = 1; i < totalDraws; i++) {
      state = advanceMascotSequence(state, lcg);
      allDraws.push(state.currentId);
    }

    expect(allDraws).toHaveLength(totalDraws);

    const cycles = totalDraws / 20;
    for (let c = 0; c < cycles; c++) {
      const slice = allDraws.slice(c * 20, (c + 1) * 20);
      expect(new Set(slice).size).toBe(20);
      for (const id of MASCOT_SEQUENCE_IDS) {
        expect(slice).toContain(id);
      }
    }

    for (let i = 0; i < allDraws.length - 1; i++) {
      expect(allDraws[i]).not.toBe(allDraws[i + 1]);
    }
  });
});
