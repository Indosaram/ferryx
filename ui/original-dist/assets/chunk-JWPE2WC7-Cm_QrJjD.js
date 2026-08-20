import { n as __name } from "./chunk-Y2CYZVJY-DUyCWguD.js";
function populateCommonDb(ast, db) {
	if (ast.accDescr) db.setAccDescription?.(ast.accDescr);
	if (ast.accTitle) db.setAccTitle?.(ast.accTitle);
	if (ast.title) db.setDiagramTitle?.(ast.title);
}
__name(populateCommonDb, "populateCommonDb");
export { populateCommonDb as t };
