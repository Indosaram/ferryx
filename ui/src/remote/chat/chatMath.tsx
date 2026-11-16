import rehypeKatex from "rehype-katex";
import remarkMath from "remark-math";
import { PluggableList } from "unified";
import "katex/dist/katex.min.css";

export const chatMathRehypePlugins: PluggableList = [[rehypeKatex, { throwOnError: false, strict: false }]];
export { remarkMath };
