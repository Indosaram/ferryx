import { r as findAndReplace } from "./lib-CtirWBBB.js";
function newlineToBreak(tree) {
	findAndReplace(tree, [/\r?\n|\r/g, replace]);
}
function replace() {
	return { type: "break" };
}
function remarkBreaks() {
	return function(tree) {
		newlineToBreak(tree);
	};
}
export { remarkBreaks as t };
