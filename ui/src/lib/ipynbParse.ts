export type NotebookCell = {
  kind: "markdown" | "code" | "raw";
  source: string;
  executionCount: number | null;
  outputs: NotebookOutput[];
};

export type NotebookOutput =
  | { kind: "text"; text: string }
  | { kind: "image"; mime: "image/png" | "image/jpeg"; base64: string }
  | { kind: "error"; name: string; message: string };

export type ParsedNotebook = {
  cells: NotebookCell[];
  language: string;
};

const ANSI_REGEX = /\u001b\[[0-9;?]*[a-zA-Z]/g;

export function stripAnsi(text: string): string {
  return text.replace(ANSI_REGEX, "");
}

function normalizeSource(source: unknown): string {
  if (typeof source === "string") {
    return source;
  }
  if (Array.isArray(source)) {
    return source.map((line) => (typeof line === "string" ? line : String(line ?? ""))).join("");
  }
  return "";
}

function parseStreamOutput(raw: Record<string, unknown>): NotebookOutput | null {
  // Stream output carries the kernel's raw terminal colors; the preview is plain text.
  const text = stripAnsi(normalizeSource(raw.text));
  return {
    kind: "text",
    text,
  };
}

function parseDisplayOrExecuteOutput(raw: Record<string, unknown>): NotebookOutput | null {
  const data = raw.data;
  if (!data || typeof data !== "object" || Array.isArray(data)) {
    return null;
  }
  const dataRecord = data as Record<string, unknown>;

  if (dataRecord["image/png"] !== undefined) {
    const pngVal = normalizeSource(dataRecord["image/png"]).replace(/\s+/g, "");
    return {
      kind: "image",
      mime: "image/png",
      base64: pngVal,
    };
  }

  if (dataRecord["image/jpeg"] !== undefined) {
    const jpegVal = normalizeSource(dataRecord["image/jpeg"]).replace(/\s+/g, "");
    return {
      kind: "image",
      mime: "image/jpeg",
      base64: jpegVal,
    };
  }

  if (dataRecord["text/plain"] !== undefined) {
    const textVal = normalizeSource(dataRecord["text/plain"]);
    return {
      kind: "text",
      text: textVal,
    };
  }

  return null;
}

function parseErrorOutput(raw: Record<string, unknown>): NotebookOutput | null {
  const name = typeof raw.ename === "string" ? raw.ename : "Error";
  let message = "";

  if (Array.isArray(raw.traceback) && raw.traceback.length > 0) {
    message = raw.traceback.map((line) => normalizeSource(line)).join("\n");
  } else if (typeof raw.evalue === "string") {
    message = raw.evalue;
  }

  return {
    kind: "error",
    name: stripAnsi(name),
    message: stripAnsi(message),
  };
}

function parseOutputItem(rawOutput: unknown): NotebookOutput | null {
  if (!rawOutput || typeof rawOutput !== "object" || Array.isArray(rawOutput)) {
    return null;
  }
  const output = rawOutput as Record<string, unknown>;
  const outputType = output.output_type;

  if (outputType === "stream") {
    return parseStreamOutput(output);
  }
  if (outputType === "execute_result" || outputType === "display_data") {
    return parseDisplayOrExecuteOutput(output);
  }
  if (outputType === "error") {
    return parseErrorOutput(output);
  }

  return null;
}

function parseCell(rawCell: unknown): NotebookCell | null {
  if (!rawCell || typeof rawCell !== "object" || Array.isArray(rawCell)) {
    return null;
  }
  const cell = rawCell as Record<string, unknown>;
  const cellType = cell.cell_type;

  let kind: "markdown" | "code" | "raw";
  if (cellType === "markdown") {
    kind = "markdown";
  } else if (cellType === "code") {
    kind = "code";
  } else if (cellType === "raw") {
    kind = "raw";
  } else {
    return null;
  }

  const source = normalizeSource(cell.source);

  let executionCount: number | null = null;
  if (kind === "code") {
    if (typeof cell.execution_count === "number") {
      executionCount = cell.execution_count;
    } else {
      executionCount = null;
    }
  }

  const outputs: NotebookOutput[] = [];
  if (kind === "code" && Array.isArray(cell.outputs)) {
    for (const rawOutput of cell.outputs) {
      const parsedOutput = parseOutputItem(rawOutput);
      if (parsedOutput) {
        outputs.push(parsedOutput);
      }
    }
  }

  return {
    kind,
    source,
    executionCount,
    outputs,
  };
}

export function parseNotebook(text: string): ParsedNotebook | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    return null;
  }

  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    return null;
  }

  const obj = parsed as Record<string, unknown>;
  // nbformat v3 nests cells under worksheets[0].cells.
  let rawCells: unknown = obj.cells;
  if (!Array.isArray(rawCells) && Array.isArray(obj.worksheets)) {
    const sheet: unknown = obj.worksheets[0];
    if (sheet && typeof sheet === "object" && !Array.isArray(sheet)) {
      rawCells = (sheet as Record<string, unknown>).cells;
    }
  }
  if (!Array.isArray(rawCells)) {
    return null;
  }

  let language = "python";
  if (obj.metadata && typeof obj.metadata === "object" && !Array.isArray(obj.metadata)) {
    const meta = obj.metadata as Record<string, unknown>;
    if (meta.kernelspec && typeof meta.kernelspec === "object" && !Array.isArray(meta.kernelspec)) {
      const kernel = meta.kernelspec as Record<string, unknown>;
      if (typeof kernel.language === "string" && kernel.language.trim() !== "") {
        language = kernel.language;
      }
    } else if (meta.language_info && typeof meta.language_info === "object" && !Array.isArray(meta.language_info)) {
      const langInfo = meta.language_info as Record<string, unknown>;
      if (typeof langInfo.name === "string" && langInfo.name.trim() !== "") {
        language = langInfo.name;
      }
    }
  }

  const cells: NotebookCell[] = [];
  for (const rawCell of rawCells) {
    const cell = parseCell(rawCell);
    if (cell) {
      cells.push(cell);
    }
  }

  return {
    cells,
    language,
  };
}
