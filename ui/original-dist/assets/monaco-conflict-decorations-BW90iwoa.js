function getLineEndColumn(line) {
	return line.length + 1;
}
function makeWholeLineRange(startLineNumber, endLineNumber) {
	return {
		startLineNumber,
		startColumn: 1,
		endLineNumber,
		endColumn: 1
	};
}
function makeMarkerRange(lineNumber, line) {
	return {
		startLineNumber: lineNumber,
		startColumn: 1,
		endLineNumber: lineNumber,
		endColumn: getLineEndColumn(line)
	};
}
function makeMarkerDecoration(lineNumber, line, label) {
	return {
		range: makeMarkerRange(lineNumber, line),
		options: {
			isWholeLine: true,
			className: "orca-conflict-marker-line",
			linesDecorationsClassName: "orca-conflict-line-decoration",
			marginClassName: "orca-conflict-margin",
			hoverMessage: { value: label },
			linesDecorationsTooltip: label,
			after: {
				content: ` ${label}`,
				inlineClassName: "orca-conflict-marker-label"
			}
		}
	};
}
function makeSectionDecoration(startLineNumber, endLineNumber, section) {
	if (startLineNumber > endLineNumber) return null;
	return {
		range: makeWholeLineRange(startLineNumber, endLineNumber),
		options: {
			isWholeLine: true,
			className: `orca-conflict-section-line orca-conflict-${section}-line`
		}
	};
}
function findGitConflictBlocks(content) {
	return parseGitConflictBlocks(content).map(({ startLine, baseLine, separatorLine, endLine }) => ({
		startLine,
		...baseLine === void 0 ? {} : { baseLine },
		separatorLine,
		endLine
	}));
}
function getGitConflictMarkerLineLength(content, lineNumber) {
	if (!Number.isInteger(lineNumber) || lineNumber < 1) return 0;
	let foundLength = 0;
	forEachLine(content, (lineStart, lineEnd, currentLineNumber) => {
		if (currentLineNumber !== lineNumber) return;
		foundLength = lineEnd - lineStart;
		return false;
	});
	return foundLength;
}
function parseGitConflictBlocks(content) {
	const blocks = [];
	let current = null;
	forEachLine(content, (lineStart, lineEnd, lineNumber) => {
		if (lineStartsWith(content, lineStart, lineEnd, "<<<<<<<")) {
			current = {
				startLine: lineNumber,
				startText: content.slice(lineStart, lineEnd)
			};
			return;
		}
		if (!current) return;
		if (lineStartsWith(content, lineStart, lineEnd, "|||||||")) {
			current.baseLine = lineNumber;
			current.baseText = content.slice(lineStart, lineEnd);
			return;
		}
		if (lineEquals(content, lineStart, lineEnd, "=======")) {
			current.separatorLine = lineNumber;
			current.separatorText = "=======";
			return;
		}
		if (lineStartsWith(content, lineStart, lineEnd, ">>>>>>>")) {
			if (current.separatorLine && current.separatorText) blocks.push({
				startLine: current.startLine,
				startText: current.startText,
				baseLine: current.baseLine,
				baseText: current.baseText,
				separatorLine: current.separatorLine,
				separatorText: current.separatorText,
				endLine: lineNumber,
				endText: content.slice(lineStart, lineEnd)
			});
			current = null;
		}
	});
	return blocks;
}
function hasGitConflictMarkers(content) {
	let found = false;
	forEachLine(content, (lineStart, lineEnd) => {
		found = lineStartsWith(content, lineStart, lineEnd, "<<<<<<<") || lineStartsWith(content, lineStart, lineEnd, "|||||||") || lineEquals(content, lineStart, lineEnd, "=======") || lineStartsWith(content, lineStart, lineEnd, ">>>>>>>");
		return found ? false : void 0;
	});
	return found;
}
function buildGitConflictDecorations(content) {
	const decorations = [];
	for (const block of parseGitConflictBlocks(content)) {
		const currentEndLine = (block.baseLine ?? block.separatorLine) - 1;
		const baseStartLine = block.baseLine ? block.baseLine + 1 : null;
		const sectionDecorations = [
			makeSectionDecoration(block.startLine + 1, currentEndLine, "current"),
			baseStartLine ? makeSectionDecoration(baseStartLine, block.separatorLine - 1, "base") : null,
			makeSectionDecoration(block.separatorLine + 1, block.endLine - 1, "incoming")
		];
		for (const decoration of sectionDecorations) if (decoration) decorations.push(decoration);
		decorations.push(makeMarkerDecoration(block.startLine, block.startText, "Current change"), ...block.baseLine ? [makeMarkerDecoration(block.baseLine, block.baseText ?? "", "Common ancestor")] : [], makeMarkerDecoration(block.separatorLine, block.separatorText, "Incoming change"), makeMarkerDecoration(block.endLine, block.endText, "End conflict"));
	}
	return decorations;
}
function forEachLine(content, visit) {
	let lineStart = 0;
	let lineNumber = 1;
	for (let index = 0; index <= content.length; index += 1) {
		if (index < content.length && content.charCodeAt(index) !== 10) continue;
		const lineEnd = index > lineStart && content.charCodeAt(index - 1) === 13 ? index - 1 : index;
		if (visit(lineStart, lineEnd, lineNumber) === false) return;
		lineStart = index + 1;
		lineNumber += 1;
	}
}
function lineStartsWith(content, lineStart, lineEnd, prefix) {
	return lineEnd - lineStart >= prefix.length && content.startsWith(prefix, lineStart);
}
function lineEquals(content, lineStart, lineEnd, value) {
	return lineEnd - lineStart === value.length && content.startsWith(value, lineStart);
}
export { hasGitConflictMarkers as i, findGitConflictBlocks as n, getGitConflictMarkerLineLength as r, buildGitConflictDecorations as t };
