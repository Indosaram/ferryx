import { describe, expect, it } from "vitest";
import { detectDelimiter, parseDelimited } from "./csvParse";

describe("detectDelimiter", () => {
  it("detects tsv from .tsv extension", () => {
    expect(detectDelimiter("data.tsv", "a,b,c")).toBe("\t");
    expect(detectDelimiter("UPPER.TSV", "a,b,c")).toBe("\t");
  });

  it("detects tsv when first line has more tabs than commas", () => {
    expect(detectDelimiter("data.txt", "col1\tcol2\tcol3\na,b")).toBe("\t");
    expect(detectDelimiter("unknown", "col1\tcol2\tcol3")).toBe("\t");
  });

  it("detects csv when first line has more commas or equal", () => {
    expect(detectDelimiter("data.csv", "col1,col2,col3")).toBe(",");
    expect(detectDelimiter("data.txt", "col1,col2\tcol3")).toBe(",");
    expect(detectDelimiter("data.txt", "plain single line")).toBe(",");
  });
});

describe("parseDelimited", () => {
  it("returns empty array for empty input", () => {
    expect(parseDelimited("", ",")).toEqual([]);
  });

  it("parses simple unquoted fields", () => {
    const csv = "a,b,c\n1,2,3";
    expect(parseDelimited(csv, ",")).toEqual([
      ["a", "b", "c"],
      ["1", "2", "3"],
    ]);
  });

  it("handles quoted comma inside fields", () => {
    const csv = 'name,address,age\n"Doe, Jane","123 Main St, Apt 4",30';
    expect(parseDelimited(csv, ",")).toEqual([
      ["name", "address", "age"],
      ["Doe, Jane", "123 Main St, Apt 4", "30"],
    ]);
  });

  it("handles embedded newline inside quotes", () => {
    const csv = 'title,description\nItem 1,"Line 1\nLine 2\nLine 3"';
    expect(parseDelimited(csv, ",")).toEqual([
      ["title", "description"],
      ["Item 1", "Line 1\nLine 2\nLine 3"],
    ]);
  });

  it("handles escaped quote with consecutive double quotes", () => {
    const csv = 'quote,author\n"He said, ""Hello!""",Alice';
    expect(parseDelimited(csv, ",")).toEqual([
      ["quote", "author"],
      ['He said, "Hello!"', "Alice"],
    ]);
  });

  it("handles CRLF and LF newlines", () => {
    const csv = "col1,col2\r\nrow1,val1\nrow2,val2\r\nrow3,val3";
    expect(parseDelimited(csv, ",")).toEqual([
      ["col1", "col2"],
      ["row1", "val1"],
      ["row2", "val2"],
      ["row3", "val3"],
    ]);
  });

  it("ignores trailing newline without generating extra empty row", () => {
    const csv = "a,b\n1,2\n";
    expect(parseDelimited(csv, ",")).toEqual([
      ["a", "b"],
      ["1", "2"],
    ]);

    const crlfCsv = "a,b\r\n1,2\r\n";
    expect(parseDelimited(crlfCsv, ",")).toEqual([
      ["a", "b"],
      ["1", "2"],
    ]);
  });

  it("parses TSV using tab delimiter", () => {
    const tsv = "col1\tcol2\n\"val\t1\"\tval2\n";
    expect(parseDelimited(tsv, "\t")).toEqual([
      ["col1", "col2"],
      ["val\t1", "val2"],
    ]);
  });

  it("treats a quote inside an unquoted field as a literal character", () => {
    expect(parseDelimited('5" pipe,10\nnext,row', ",")).toEqual([
      ['5" pipe', "10"],
      ["next", "row"],
    ]);
  });
});
