import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { FilePreviewCsv } from "./FilePreviewCsv";

afterEach(() => {
  cleanup();
});

describe("FilePreviewCsv", () => {
  it("renders header row as th elements and body rows with row numbers", () => {
    const csv = "Name,Age,Role\nAlice,30,Engineer\nBob,25,Designer";
    render(<FilePreviewCsv text={csv} displayName="users.csv" />);

    const table = screen.getByTestId("file-preview-csv");
    expect(table).toBeDefined();

    const headers = table.querySelectorAll("th");
    expect(headers).toHaveLength(4);
    expect(headers[0].textContent).toBe("#");
    expect(headers[1].textContent).toBe("Name");
    expect(headers[2].textContent).toBe("Age");
    expect(headers[3].textContent).toBe("Role");

    const rows = table.querySelectorAll("tbody tr");
    expect(rows).toHaveLength(2);

    const firstRowCells = rows[0].querySelectorAll("td");
    expect(firstRowCells[0].textContent).toBe("1");
    expect(firstRowCells[1].textContent).toBe("Alice");
    expect(firstRowCells[2].textContent).toBe("30");
    expect(firstRowCells[3].textContent).toBe("Engineer");

    const secondRowCells = rows[1].querySelectorAll("td");
    expect(secondRowCells[0].textContent).toBe("2");
    expect(secondRowCells[1].textContent).toBe("Bob");
    expect(secondRowCells[2].textContent).toBe("25");
    expect(secondRowCells[3].textContent).toBe("Designer");
  });

  it("handles TSV format by detecting .tsv extension or delimiter", () => {
    const tsv = "Header1\tHeader2\nValue1\tValue2";
    render(<FilePreviewCsv text={tsv} displayName="report.tsv" />);

    const table = screen.getByTestId("file-preview-csv");
    const headers = table.querySelectorAll("th");
    expect(headers[1].textContent).toBe("Header1");
    expect(headers[2].textContent).toBe("Header2");

    const cells = table.querySelectorAll("tbody td");
    expect(cells[0].textContent).toBe("1");
    expect(cells[1].textContent).toBe("Value1");
    expect(cells[2].textContent).toBe("Value2");
  });

  it("handles quoted commas and embedded newlines in table cells", () => {
    const csv = 'Title,Desc\nBook,"A, B, and C"\nMulti,"Line 1\nLine 2"';
    render(<FilePreviewCsv text={csv} displayName="books.csv" />);

    const rows = screen.getByTestId("file-preview-csv").querySelectorAll("tbody tr");
    expect(rows).toHaveLength(2);

    const row1 = rows[0].querySelectorAll("td");
    expect(row1[1].textContent).toBe("Book");
    expect(row1[2].textContent).toBe("A, B, and C");

    const row2 = rows[1].querySelectorAll("td");
    expect(row2[1].textContent).toBe("Multi");
    expect(row2[2].textContent).toBe("Line 1\nLine 2");
  });

  it("displays truncation footer when body row count exceeds 5000", () => {
    const lines = ["id,value"];
    for (let i = 1; i <= 5001; i++) {
      lines.push(`${i},val${i}`);
    }
    const csv = lines.join("\n");

    render(<FilePreviewCsv text={csv} displayName="big.csv" />);

    const truncationMessage = screen.getByTestId("file-preview-csv-truncated");
    expect(truncationMessage).toBeDefined();
    expect(truncationMessage.textContent).toBe("Showing 5000 of 5001 rows");

    const renderedRows = screen.getByTestId("file-preview-csv").querySelectorAll("tbody tr");
    expect(renderedRows).toHaveLength(5000);
  });

  it("toggles between table view and raw text view", () => {
    const csv = "col1,col2\nval1,val2";
    render(<FilePreviewCsv text={csv} displayName="test.csv" />);

    expect(screen.getByTestId("file-preview-csv")).toBeDefined();
    expect(screen.queryByTestId("file-preview-csv-raw")).toBeNull();

    const toggleBtn = screen.getByTestId("file-preview-csv-raw-toggle");
    expect(toggleBtn.textContent).toBe("Raw");

    fireEvent.click(toggleBtn);

    expect(screen.queryByTestId("file-preview-csv")).toBeNull();
    const rawView = screen.getByTestId("file-preview-csv-raw");
    expect(rawView).toBeDefined();
    expect(rawView.textContent).toBe(csv);
    expect(toggleBtn.textContent).toBe("Table");

    fireEvent.click(toggleBtn);

    expect(screen.getByTestId("file-preview-csv")).toBeDefined();
    expect(screen.queryByTestId("file-preview-csv-raw")).toBeNull();
    expect(toggleBtn.textContent).toBe("Raw");
  });

  it("keeps both the table and raw views text-selectable", () => {
    render(<FilePreviewCsv text={"a,b\n1,2"} displayName="t.csv" />);
    expect(screen.getByTestId("file-preview-csv")).toHaveClass("selectable");
    fireEvent.click(screen.getByTestId("file-preview-csv-raw-toggle"));
    expect(screen.getByTestId("file-preview-csv-raw")).toHaveClass("selectable");
  });
});
