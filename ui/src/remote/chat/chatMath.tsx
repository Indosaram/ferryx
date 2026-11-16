import rehypeKatex from "rehype-katex";
import remarkMath from "remark-math";
import "katex/dist/katex.min.css";

export const rehypeKatexNonThrowing = [[rehypeKatex, { throwOnError: false, strict: false }]];
export const rehypeKatexPlugin = rehypeKatexNonThrowing;
export { remarkMath };
