import React from "react";

interface MarkdownNode {
  type: string;
  value?: string;
  children?: MarkdownNode[];
  position?: { start: { offset?: number }; end: { offset?: number } };
  data?: Record<string, unknown>;
}

/** A deliberately small delimiter plugin for the existing chat markdown pipeline. */
export function remarkChatMath() {
  return (tree: MarkdownNode, file: { value: unknown }) => {
    const source = String(file.value);
    const visit = (parent: MarkdownNode) => {
      if (!parent.children || ["code", "inlineCode", "html"].includes(parent.type)) return;
      parent.children = parent.children.flatMap((node): MarkdownNode[] => {
        if (node.type !== "text" || !node.value) {
          visit(node);
          return [node];
        }
        // Source offsets retain escaped dollars which markdown has already unescaped.
        const raw = source.slice(node.position?.start?.offset, node.position?.end?.offset);
        if (raw.includes("\\$")) return [node];
        const result: MarkdownNode[] = [];
        const pattern = /\$\$([\s\S]+?)\$\$|\$([^$\n]+?)\$/g;
        const rawMatches = [...raw.matchAll(new RegExp(pattern))];
        let start = 0;
        let matchIndex = 0;
        for (const match of node.value.matchAll(pattern)) {
          const original = rawMatches[matchIndex++] ?? match;
          const index = match.index!;
          if (index > start) result.push({ type: "text", value: node.value.slice(start, index) });
          result.push({
            type: "chatMath",
            data: {
              hName: "span",
              hProperties: { "data-chat-math": match[1] !== undefined ? "block" : "inline", "data-math-source": original[1] ?? original[2] },
            },
          });
          start = index + match[0].length;
        }
        if (!result.length) return [node];
        if (start < node.value.length) result.push({ type: "text", value: node.value.slice(start) });
        return result;
      });
    };
    visit(tree);
  };
}

// React 18's JSX types predate MathML; createElement still creates native MathML nodes.
const element = (tag: string, ...children: React.ReactNode[]) => React.createElement(tag, null, ...children);
const space = () => React.createElement("mspace", { width: "0.2em" });

const SYMBOLS: Record<string, string> = {
  int: "∫", sum: "∑", prod: "∏", infinity: "∞", infty: "∞",
  alpha: "α", beta: "β", gamma: "γ", delta: "δ", theta: "θ", lambda: "λ",
  mu: "μ", pi: "π", sigma: "σ", phi: "φ", omega: "ω",
  times: "×", cdot: "·", le: "≤", leq: "≤", ge: "≥", geq: "≥", neq: "≠", pm: "±",
};

/** Native MathML, not KaTeX: basic symbols, groups, fractions, roots and scripts. */
export function ChatMath({ source, display = false }: { source: string; display?: boolean }) {
  let cursor = 0;
  const atom = (): React.ReactNode => {
    const char = source[cursor++];
    if (char === "{") {
      const children = sequence("}");
      if (source[cursor++] !== "}") throw new Error("Unclosed math group");
      return element("mrow", children);
    }
    if (char === "\\") {
      const command = /^[a-zA-Z]+/.exec(source.slice(cursor))?.[0];
      if (!command) {
        const escaped = source[cursor++];
        if ([",", ";", " ", "!"].includes(escaped)) return space();
        if (["{", "}", "$", "%", "_"].includes(escaped)) return element("mo", escaped);
        throw new Error("Unsupported math escape");
      }
      cursor += command.length;
      if (command === "frac") return element("mfrac", atom(), atom());
      if (command === "sqrt") return element("msqrt", atom());
      if (!SYMBOLS[command]) throw new Error("Unsupported math command");
      return element("mo", SYMBOLS[command]);
    }
    if (!char || char === "}" || char === "^" || char === "_") throw new Error("Missing math operand");
    if (/\d/.test(char)) return element("mn", char);
    if (/[a-zA-Z]/.test(char)) return element("mi", char);
    if (/\s/.test(char)) return space();
    return element("mo", char);
  };
  const sequence = (end?: string): React.ReactNode[] => {
    const nodes: React.ReactNode[] = [];
    while (cursor < source.length && source[cursor] !== end) {
      let base = atom();
      let sub: React.ReactNode;
      let sup: React.ReactNode;
      while (source[cursor] === "^" || source[cursor] === "_") {
        const script = source[cursor++];
        if (script === "^") sup = atom();
        else sub = atom();
      }
      if (sub && sup) base = element("msubsup", base, sub, sup);
      else if (sub) base = element("msub", base, sub);
      else if (sup) base = element("msup", base, sup);
      nodes.push(<React.Fragment key={nodes.length}>{base}</React.Fragment>);
    }
    return nodes;
  };
  try {
    const body = sequence();
    // Compatibility hooks for existing math probes; rendering uses browser MathML only.
    return <span className={display ? "chat-math katex-display" : "chat-math katex"} style={display ? { display: "block", overflowX: "auto", margin: "0.75em 0" } : undefined}>
      {React.createElement("math", { display: display ? "block" : "inline", "aria-label": source }, element("mrow", body))}
    </span>;
  } catch {
    // Unsupported or incomplete expressions keep their exact source instead of vanishing.
    return <>{display ? `$$${source}$$` : `$${source}$`}</>;
  }
}
