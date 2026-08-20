import { a as MarkerSeverity, c as Range, f as Uri, h as languages, p as editor } from "./editor.api2-DX_-Ye6K.js";
var DocumentUri;
(function(DocumentUri$1) {
	function is(value) {
		return typeof value === "string";
	}
	DocumentUri$1.is = is;
})(DocumentUri || (DocumentUri = {}));
var URI;
(function(URI$1) {
	function is(value) {
		return typeof value === "string";
	}
	URI$1.is = is;
})(URI || (URI = {}));
var integer;
(function(integer$1) {
	integer$1.MIN_VALUE = -2147483648;
	integer$1.MAX_VALUE = 2147483647;
	function is(value) {
		return typeof value === "number" && integer$1.MIN_VALUE <= value && value <= integer$1.MAX_VALUE;
	}
	integer$1.is = is;
})(integer || (integer = {}));
var uinteger;
(function(uinteger$1) {
	uinteger$1.MIN_VALUE = 0;
	uinteger$1.MAX_VALUE = 2147483647;
	function is(value) {
		return typeof value === "number" && uinteger$1.MIN_VALUE <= value && value <= uinteger$1.MAX_VALUE;
	}
	uinteger$1.is = is;
})(uinteger || (uinteger = {}));
var Position;
(function(Position$1) {
	function create(line, character) {
		if (line === Number.MAX_VALUE) line = uinteger.MAX_VALUE;
		if (character === Number.MAX_VALUE) character = uinteger.MAX_VALUE;
		return {
			line,
			character
		};
	}
	Position$1.create = create;
	function is(value) {
		let candidate = value;
		return Is.objectLiteral(candidate) && Is.uinteger(candidate.line) && Is.uinteger(candidate.character);
	}
	Position$1.is = is;
})(Position || (Position = {}));
var Range$1;
(function(Range$2) {
	function create(one, two, three, four) {
		if (Is.uinteger(one) && Is.uinteger(two) && Is.uinteger(three) && Is.uinteger(four)) return {
			start: Position.create(one, two),
			end: Position.create(three, four)
		};
		else if (Position.is(one) && Position.is(two)) return {
			start: one,
			end: two
		};
		else throw new Error(`Range#create called with invalid arguments[${one}, ${two}, ${three}, ${four}]`);
	}
	Range$2.create = create;
	function is(value) {
		let candidate = value;
		return Is.objectLiteral(candidate) && Position.is(candidate.start) && Position.is(candidate.end);
	}
	Range$2.is = is;
})(Range$1 || (Range$1 = {}));
var Location;
(function(Location$1) {
	function create(uri, range) {
		return {
			uri,
			range
		};
	}
	Location$1.create = create;
	function is(value) {
		let candidate = value;
		return Is.objectLiteral(candidate) && Range$1.is(candidate.range) && (Is.string(candidate.uri) || Is.undefined(candidate.uri));
	}
	Location$1.is = is;
})(Location || (Location = {}));
var LocationLink;
(function(LocationLink$1) {
	function create(targetUri, targetRange, targetSelectionRange, originSelectionRange) {
		return {
			targetUri,
			targetRange,
			targetSelectionRange,
			originSelectionRange
		};
	}
	LocationLink$1.create = create;
	function is(value) {
		let candidate = value;
		return Is.objectLiteral(candidate) && Range$1.is(candidate.targetRange) && Is.string(candidate.targetUri) && Range$1.is(candidate.targetSelectionRange) && (Range$1.is(candidate.originSelectionRange) || Is.undefined(candidate.originSelectionRange));
	}
	LocationLink$1.is = is;
})(LocationLink || (LocationLink = {}));
var Color;
(function(Color$1) {
	function create(red, green, blue, alpha) {
		return {
			red,
			green,
			blue,
			alpha
		};
	}
	Color$1.create = create;
	function is(value) {
		const candidate = value;
		return Is.objectLiteral(candidate) && Is.numberRange(candidate.red, 0, 1) && Is.numberRange(candidate.green, 0, 1) && Is.numberRange(candidate.blue, 0, 1) && Is.numberRange(candidate.alpha, 0, 1);
	}
	Color$1.is = is;
})(Color || (Color = {}));
var ColorInformation;
(function(ColorInformation$1) {
	function create(range, color) {
		return {
			range,
			color
		};
	}
	ColorInformation$1.create = create;
	function is(value) {
		const candidate = value;
		return Is.objectLiteral(candidate) && Range$1.is(candidate.range) && Color.is(candidate.color);
	}
	ColorInformation$1.is = is;
})(ColorInformation || (ColorInformation = {}));
var ColorPresentation;
(function(ColorPresentation$1) {
	function create(label, textEdit, additionalTextEdits) {
		return {
			label,
			textEdit,
			additionalTextEdits
		};
	}
	ColorPresentation$1.create = create;
	function is(value) {
		const candidate = value;
		return Is.objectLiteral(candidate) && Is.string(candidate.label) && (Is.undefined(candidate.textEdit) || TextEdit.is(candidate)) && (Is.undefined(candidate.additionalTextEdits) || Is.typedArray(candidate.additionalTextEdits, TextEdit.is));
	}
	ColorPresentation$1.is = is;
})(ColorPresentation || (ColorPresentation = {}));
var FoldingRangeKind;
(function(FoldingRangeKind$1) {
	FoldingRangeKind$1.Comment = "comment";
	FoldingRangeKind$1.Imports = "imports";
	FoldingRangeKind$1.Region = "region";
})(FoldingRangeKind || (FoldingRangeKind = {}));
var FoldingRange;
(function(FoldingRange$1) {
	function create(startLine, endLine, startCharacter, endCharacter, kind, collapsedText) {
		const result = {
			startLine,
			endLine
		};
		if (Is.defined(startCharacter)) result.startCharacter = startCharacter;
		if (Is.defined(endCharacter)) result.endCharacter = endCharacter;
		if (Is.defined(kind)) result.kind = kind;
		if (Is.defined(collapsedText)) result.collapsedText = collapsedText;
		return result;
	}
	FoldingRange$1.create = create;
	function is(value) {
		const candidate = value;
		return Is.objectLiteral(candidate) && Is.uinteger(candidate.startLine) && Is.uinteger(candidate.startLine) && (Is.undefined(candidate.startCharacter) || Is.uinteger(candidate.startCharacter)) && (Is.undefined(candidate.endCharacter) || Is.uinteger(candidate.endCharacter)) && (Is.undefined(candidate.kind) || Is.string(candidate.kind));
	}
	FoldingRange$1.is = is;
})(FoldingRange || (FoldingRange = {}));
var DiagnosticRelatedInformation;
(function(DiagnosticRelatedInformation$1) {
	function create(location, message) {
		return {
			location,
			message
		};
	}
	DiagnosticRelatedInformation$1.create = create;
	function is(value) {
		let candidate = value;
		return Is.defined(candidate) && Location.is(candidate.location) && Is.string(candidate.message);
	}
	DiagnosticRelatedInformation$1.is = is;
})(DiagnosticRelatedInformation || (DiagnosticRelatedInformation = {}));
var DiagnosticSeverity;
(function(DiagnosticSeverity$1) {
	DiagnosticSeverity$1.Error = 1;
	DiagnosticSeverity$1.Warning = 2;
	DiagnosticSeverity$1.Information = 3;
	DiagnosticSeverity$1.Hint = 4;
})(DiagnosticSeverity || (DiagnosticSeverity = {}));
var DiagnosticTag;
(function(DiagnosticTag$1) {
	DiagnosticTag$1.Unnecessary = 1;
	DiagnosticTag$1.Deprecated = 2;
})(DiagnosticTag || (DiagnosticTag = {}));
var CodeDescription;
(function(CodeDescription$1) {
	function is(value) {
		const candidate = value;
		return Is.objectLiteral(candidate) && Is.string(candidate.href);
	}
	CodeDescription$1.is = is;
})(CodeDescription || (CodeDescription = {}));
var Diagnostic;
(function(Diagnostic$1) {
	function create(range, message, severity, code, source, relatedInformation) {
		let result = {
			range,
			message
		};
		if (Is.defined(severity)) result.severity = severity;
		if (Is.defined(code)) result.code = code;
		if (Is.defined(source)) result.source = source;
		if (Is.defined(relatedInformation)) result.relatedInformation = relatedInformation;
		return result;
	}
	Diagnostic$1.create = create;
	function is(value) {
		var _a;
		let candidate = value;
		return Is.defined(candidate) && Range$1.is(candidate.range) && Is.string(candidate.message) && (Is.number(candidate.severity) || Is.undefined(candidate.severity)) && (Is.integer(candidate.code) || Is.string(candidate.code) || Is.undefined(candidate.code)) && (Is.undefined(candidate.codeDescription) || Is.string((_a = candidate.codeDescription) === null || _a === void 0 ? void 0 : _a.href)) && (Is.string(candidate.source) || Is.undefined(candidate.source)) && (Is.undefined(candidate.relatedInformation) || Is.typedArray(candidate.relatedInformation, DiagnosticRelatedInformation.is));
	}
	Diagnostic$1.is = is;
})(Diagnostic || (Diagnostic = {}));
var Command;
(function(Command$1) {
	function create(title, command, ...args) {
		let result = {
			title,
			command
		};
		if (Is.defined(args) && args.length > 0) result.arguments = args;
		return result;
	}
	Command$1.create = create;
	function is(value) {
		let candidate = value;
		return Is.defined(candidate) && Is.string(candidate.title) && Is.string(candidate.command);
	}
	Command$1.is = is;
})(Command || (Command = {}));
var TextEdit;
(function(TextEdit$1) {
	function replace(range, newText) {
		return {
			range,
			newText
		};
	}
	TextEdit$1.replace = replace;
	function insert(position, newText) {
		return {
			range: {
				start: position,
				end: position
			},
			newText
		};
	}
	TextEdit$1.insert = insert;
	function del(range) {
		return {
			range,
			newText: ""
		};
	}
	TextEdit$1.del = del;
	function is(value) {
		const candidate = value;
		return Is.objectLiteral(candidate) && Is.string(candidate.newText) && Range$1.is(candidate.range);
	}
	TextEdit$1.is = is;
})(TextEdit || (TextEdit = {}));
var ChangeAnnotation;
(function(ChangeAnnotation$1) {
	function create(label, needsConfirmation, description) {
		const result = { label };
		if (needsConfirmation !== void 0) result.needsConfirmation = needsConfirmation;
		if (description !== void 0) result.description = description;
		return result;
	}
	ChangeAnnotation$1.create = create;
	function is(value) {
		const candidate = value;
		return Is.objectLiteral(candidate) && Is.string(candidate.label) && (Is.boolean(candidate.needsConfirmation) || candidate.needsConfirmation === void 0) && (Is.string(candidate.description) || candidate.description === void 0);
	}
	ChangeAnnotation$1.is = is;
})(ChangeAnnotation || (ChangeAnnotation = {}));
var ChangeAnnotationIdentifier;
(function(ChangeAnnotationIdentifier$1) {
	function is(value) {
		const candidate = value;
		return Is.string(candidate);
	}
	ChangeAnnotationIdentifier$1.is = is;
})(ChangeAnnotationIdentifier || (ChangeAnnotationIdentifier = {}));
var AnnotatedTextEdit;
(function(AnnotatedTextEdit$1) {
	function replace(range, newText, annotation) {
		return {
			range,
			newText,
			annotationId: annotation
		};
	}
	AnnotatedTextEdit$1.replace = replace;
	function insert(position, newText, annotation) {
		return {
			range: {
				start: position,
				end: position
			},
			newText,
			annotationId: annotation
		};
	}
	AnnotatedTextEdit$1.insert = insert;
	function del(range, annotation) {
		return {
			range,
			newText: "",
			annotationId: annotation
		};
	}
	AnnotatedTextEdit$1.del = del;
	function is(value) {
		const candidate = value;
		return TextEdit.is(candidate) && (ChangeAnnotation.is(candidate.annotationId) || ChangeAnnotationIdentifier.is(candidate.annotationId));
	}
	AnnotatedTextEdit$1.is = is;
})(AnnotatedTextEdit || (AnnotatedTextEdit = {}));
var TextDocumentEdit;
(function(TextDocumentEdit$1) {
	function create(textDocument, edits) {
		return {
			textDocument,
			edits
		};
	}
	TextDocumentEdit$1.create = create;
	function is(value) {
		let candidate = value;
		return Is.defined(candidate) && OptionalVersionedTextDocumentIdentifier.is(candidate.textDocument) && Array.isArray(candidate.edits);
	}
	TextDocumentEdit$1.is = is;
})(TextDocumentEdit || (TextDocumentEdit = {}));
var CreateFile;
(function(CreateFile$1) {
	function create(uri, options, annotation) {
		let result = {
			kind: "create",
			uri
		};
		if (options !== void 0 && (options.overwrite !== void 0 || options.ignoreIfExists !== void 0)) result.options = options;
		if (annotation !== void 0) result.annotationId = annotation;
		return result;
	}
	CreateFile$1.create = create;
	function is(value) {
		let candidate = value;
		return candidate && candidate.kind === "create" && Is.string(candidate.uri) && (candidate.options === void 0 || (candidate.options.overwrite === void 0 || Is.boolean(candidate.options.overwrite)) && (candidate.options.ignoreIfExists === void 0 || Is.boolean(candidate.options.ignoreIfExists))) && (candidate.annotationId === void 0 || ChangeAnnotationIdentifier.is(candidate.annotationId));
	}
	CreateFile$1.is = is;
})(CreateFile || (CreateFile = {}));
var RenameFile;
(function(RenameFile$1) {
	function create(oldUri, newUri, options, annotation) {
		let result = {
			kind: "rename",
			oldUri,
			newUri
		};
		if (options !== void 0 && (options.overwrite !== void 0 || options.ignoreIfExists !== void 0)) result.options = options;
		if (annotation !== void 0) result.annotationId = annotation;
		return result;
	}
	RenameFile$1.create = create;
	function is(value) {
		let candidate = value;
		return candidate && candidate.kind === "rename" && Is.string(candidate.oldUri) && Is.string(candidate.newUri) && (candidate.options === void 0 || (candidate.options.overwrite === void 0 || Is.boolean(candidate.options.overwrite)) && (candidate.options.ignoreIfExists === void 0 || Is.boolean(candidate.options.ignoreIfExists))) && (candidate.annotationId === void 0 || ChangeAnnotationIdentifier.is(candidate.annotationId));
	}
	RenameFile$1.is = is;
})(RenameFile || (RenameFile = {}));
var DeleteFile;
(function(DeleteFile$1) {
	function create(uri, options, annotation) {
		let result = {
			kind: "delete",
			uri
		};
		if (options !== void 0 && (options.recursive !== void 0 || options.ignoreIfNotExists !== void 0)) result.options = options;
		if (annotation !== void 0) result.annotationId = annotation;
		return result;
	}
	DeleteFile$1.create = create;
	function is(value) {
		let candidate = value;
		return candidate && candidate.kind === "delete" && Is.string(candidate.uri) && (candidate.options === void 0 || (candidate.options.recursive === void 0 || Is.boolean(candidate.options.recursive)) && (candidate.options.ignoreIfNotExists === void 0 || Is.boolean(candidate.options.ignoreIfNotExists))) && (candidate.annotationId === void 0 || ChangeAnnotationIdentifier.is(candidate.annotationId));
	}
	DeleteFile$1.is = is;
})(DeleteFile || (DeleteFile = {}));
var WorkspaceEdit;
(function(WorkspaceEdit$1) {
	function is(value) {
		let candidate = value;
		return candidate && (candidate.changes !== void 0 || candidate.documentChanges !== void 0) && (candidate.documentChanges === void 0 || candidate.documentChanges.every((change) => {
			if (Is.string(change.kind)) return CreateFile.is(change) || RenameFile.is(change) || DeleteFile.is(change);
			else return TextDocumentEdit.is(change);
		}));
	}
	WorkspaceEdit$1.is = is;
})(WorkspaceEdit || (WorkspaceEdit = {}));
var TextDocumentIdentifier;
(function(TextDocumentIdentifier$1) {
	function create(uri) {
		return { uri };
	}
	TextDocumentIdentifier$1.create = create;
	function is(value) {
		let candidate = value;
		return Is.defined(candidate) && Is.string(candidate.uri);
	}
	TextDocumentIdentifier$1.is = is;
})(TextDocumentIdentifier || (TextDocumentIdentifier = {}));
var VersionedTextDocumentIdentifier;
(function(VersionedTextDocumentIdentifier$1) {
	function create(uri, version) {
		return {
			uri,
			version
		};
	}
	VersionedTextDocumentIdentifier$1.create = create;
	function is(value) {
		let candidate = value;
		return Is.defined(candidate) && Is.string(candidate.uri) && Is.integer(candidate.version);
	}
	VersionedTextDocumentIdentifier$1.is = is;
})(VersionedTextDocumentIdentifier || (VersionedTextDocumentIdentifier = {}));
var OptionalVersionedTextDocumentIdentifier;
(function(OptionalVersionedTextDocumentIdentifier$1) {
	function create(uri, version) {
		return {
			uri,
			version
		};
	}
	OptionalVersionedTextDocumentIdentifier$1.create = create;
	function is(value) {
		let candidate = value;
		return Is.defined(candidate) && Is.string(candidate.uri) && (candidate.version === null || Is.integer(candidate.version));
	}
	OptionalVersionedTextDocumentIdentifier$1.is = is;
})(OptionalVersionedTextDocumentIdentifier || (OptionalVersionedTextDocumentIdentifier = {}));
var TextDocumentItem;
(function(TextDocumentItem$1) {
	function create(uri, languageId, version, text) {
		return {
			uri,
			languageId,
			version,
			text
		};
	}
	TextDocumentItem$1.create = create;
	function is(value) {
		let candidate = value;
		return Is.defined(candidate) && Is.string(candidate.uri) && Is.string(candidate.languageId) && Is.integer(candidate.version) && Is.string(candidate.text);
	}
	TextDocumentItem$1.is = is;
})(TextDocumentItem || (TextDocumentItem = {}));
var MarkupKind;
(function(MarkupKind$1) {
	MarkupKind$1.PlainText = "plaintext";
	MarkupKind$1.Markdown = "markdown";
	function is(value) {
		const candidate = value;
		return candidate === MarkupKind$1.PlainText || candidate === MarkupKind$1.Markdown;
	}
	MarkupKind$1.is = is;
})(MarkupKind || (MarkupKind = {}));
var MarkupContent;
(function(MarkupContent$1) {
	function is(value) {
		const candidate = value;
		return Is.objectLiteral(value) && MarkupKind.is(candidate.kind) && Is.string(candidate.value);
	}
	MarkupContent$1.is = is;
})(MarkupContent || (MarkupContent = {}));
var CompletionItemKind;
(function(CompletionItemKind$1) {
	CompletionItemKind$1.Text = 1;
	CompletionItemKind$1.Method = 2;
	CompletionItemKind$1.Function = 3;
	CompletionItemKind$1.Constructor = 4;
	CompletionItemKind$1.Field = 5;
	CompletionItemKind$1.Variable = 6;
	CompletionItemKind$1.Class = 7;
	CompletionItemKind$1.Interface = 8;
	CompletionItemKind$1.Module = 9;
	CompletionItemKind$1.Property = 10;
	CompletionItemKind$1.Unit = 11;
	CompletionItemKind$1.Value = 12;
	CompletionItemKind$1.Enum = 13;
	CompletionItemKind$1.Keyword = 14;
	CompletionItemKind$1.Snippet = 15;
	CompletionItemKind$1.Color = 16;
	CompletionItemKind$1.File = 17;
	CompletionItemKind$1.Reference = 18;
	CompletionItemKind$1.Folder = 19;
	CompletionItemKind$1.EnumMember = 20;
	CompletionItemKind$1.Constant = 21;
	CompletionItemKind$1.Struct = 22;
	CompletionItemKind$1.Event = 23;
	CompletionItemKind$1.Operator = 24;
	CompletionItemKind$1.TypeParameter = 25;
})(CompletionItemKind || (CompletionItemKind = {}));
var InsertTextFormat;
(function(InsertTextFormat$1) {
	InsertTextFormat$1.PlainText = 1;
	InsertTextFormat$1.Snippet = 2;
})(InsertTextFormat || (InsertTextFormat = {}));
var CompletionItemTag;
(function(CompletionItemTag$1) {
	CompletionItemTag$1.Deprecated = 1;
})(CompletionItemTag || (CompletionItemTag = {}));
var InsertReplaceEdit;
(function(InsertReplaceEdit$1) {
	function create(newText, insert, replace) {
		return {
			newText,
			insert,
			replace
		};
	}
	InsertReplaceEdit$1.create = create;
	function is(value) {
		const candidate = value;
		return candidate && Is.string(candidate.newText) && Range$1.is(candidate.insert) && Range$1.is(candidate.replace);
	}
	InsertReplaceEdit$1.is = is;
})(InsertReplaceEdit || (InsertReplaceEdit = {}));
var InsertTextMode;
(function(InsertTextMode$1) {
	InsertTextMode$1.asIs = 1;
	InsertTextMode$1.adjustIndentation = 2;
})(InsertTextMode || (InsertTextMode = {}));
var CompletionItemLabelDetails;
(function(CompletionItemLabelDetails$1) {
	function is(value) {
		const candidate = value;
		return candidate && (Is.string(candidate.detail) || candidate.detail === void 0) && (Is.string(candidate.description) || candidate.description === void 0);
	}
	CompletionItemLabelDetails$1.is = is;
})(CompletionItemLabelDetails || (CompletionItemLabelDetails = {}));
var CompletionItem;
(function(CompletionItem$1) {
	function create(label) {
		return { label };
	}
	CompletionItem$1.create = create;
})(CompletionItem || (CompletionItem = {}));
var CompletionList;
(function(CompletionList$1) {
	function create(items, isIncomplete) {
		return {
			items: items ? items : [],
			isIncomplete: !!isIncomplete
		};
	}
	CompletionList$1.create = create;
})(CompletionList || (CompletionList = {}));
var MarkedString;
(function(MarkedString$1) {
	function fromPlainText(plainText) {
		return plainText.replace(/[\\`*_{}[\]()#+\-.!]/g, "\\$&");
	}
	MarkedString$1.fromPlainText = fromPlainText;
	function is(value) {
		const candidate = value;
		return Is.string(candidate) || Is.objectLiteral(candidate) && Is.string(candidate.language) && Is.string(candidate.value);
	}
	MarkedString$1.is = is;
})(MarkedString || (MarkedString = {}));
var Hover;
(function(Hover$1) {
	function is(value) {
		let candidate = value;
		return !!candidate && Is.objectLiteral(candidate) && (MarkupContent.is(candidate.contents) || MarkedString.is(candidate.contents) || Is.typedArray(candidate.contents, MarkedString.is)) && (value.range === void 0 || Range$1.is(value.range));
	}
	Hover$1.is = is;
})(Hover || (Hover = {}));
var ParameterInformation;
(function(ParameterInformation$1) {
	function create(label, documentation) {
		return documentation ? {
			label,
			documentation
		} : { label };
	}
	ParameterInformation$1.create = create;
})(ParameterInformation || (ParameterInformation = {}));
var SignatureInformation;
(function(SignatureInformation$1) {
	function create(label, documentation, ...parameters) {
		let result = { label };
		if (Is.defined(documentation)) result.documentation = documentation;
		if (Is.defined(parameters)) result.parameters = parameters;
		else result.parameters = [];
		return result;
	}
	SignatureInformation$1.create = create;
})(SignatureInformation || (SignatureInformation = {}));
var DocumentHighlightKind;
(function(DocumentHighlightKind$1) {
	DocumentHighlightKind$1.Text = 1;
	DocumentHighlightKind$1.Read = 2;
	DocumentHighlightKind$1.Write = 3;
})(DocumentHighlightKind || (DocumentHighlightKind = {}));
var DocumentHighlight;
(function(DocumentHighlight$1) {
	function create(range, kind) {
		let result = { range };
		if (Is.number(kind)) result.kind = kind;
		return result;
	}
	DocumentHighlight$1.create = create;
})(DocumentHighlight || (DocumentHighlight = {}));
var SymbolKind;
(function(SymbolKind$1) {
	SymbolKind$1.File = 1;
	SymbolKind$1.Module = 2;
	SymbolKind$1.Namespace = 3;
	SymbolKind$1.Package = 4;
	SymbolKind$1.Class = 5;
	SymbolKind$1.Method = 6;
	SymbolKind$1.Property = 7;
	SymbolKind$1.Field = 8;
	SymbolKind$1.Constructor = 9;
	SymbolKind$1.Enum = 10;
	SymbolKind$1.Interface = 11;
	SymbolKind$1.Function = 12;
	SymbolKind$1.Variable = 13;
	SymbolKind$1.Constant = 14;
	SymbolKind$1.String = 15;
	SymbolKind$1.Number = 16;
	SymbolKind$1.Boolean = 17;
	SymbolKind$1.Array = 18;
	SymbolKind$1.Object = 19;
	SymbolKind$1.Key = 20;
	SymbolKind$1.Null = 21;
	SymbolKind$1.EnumMember = 22;
	SymbolKind$1.Struct = 23;
	SymbolKind$1.Event = 24;
	SymbolKind$1.Operator = 25;
	SymbolKind$1.TypeParameter = 26;
})(SymbolKind || (SymbolKind = {}));
var SymbolTag;
(function(SymbolTag$1) {
	SymbolTag$1.Deprecated = 1;
})(SymbolTag || (SymbolTag = {}));
var SymbolInformation;
(function(SymbolInformation$1) {
	function create(name, kind, range, uri, containerName) {
		let result = {
			name,
			kind,
			location: {
				uri,
				range
			}
		};
		if (containerName) result.containerName = containerName;
		return result;
	}
	SymbolInformation$1.create = create;
})(SymbolInformation || (SymbolInformation = {}));
var WorkspaceSymbol;
(function(WorkspaceSymbol$1) {
	function create(name, kind, uri, range) {
		return range !== void 0 ? {
			name,
			kind,
			location: {
				uri,
				range
			}
		} : {
			name,
			kind,
			location: { uri }
		};
	}
	WorkspaceSymbol$1.create = create;
})(WorkspaceSymbol || (WorkspaceSymbol = {}));
var DocumentSymbol;
(function(DocumentSymbol$1) {
	function create(name, detail, kind, range, selectionRange, children) {
		let result = {
			name,
			detail,
			kind,
			range,
			selectionRange
		};
		if (children !== void 0) result.children = children;
		return result;
	}
	DocumentSymbol$1.create = create;
	function is(value) {
		let candidate = value;
		return candidate && Is.string(candidate.name) && Is.number(candidate.kind) && Range$1.is(candidate.range) && Range$1.is(candidate.selectionRange) && (candidate.detail === void 0 || Is.string(candidate.detail)) && (candidate.deprecated === void 0 || Is.boolean(candidate.deprecated)) && (candidate.children === void 0 || Array.isArray(candidate.children)) && (candidate.tags === void 0 || Array.isArray(candidate.tags));
	}
	DocumentSymbol$1.is = is;
})(DocumentSymbol || (DocumentSymbol = {}));
var CodeActionKind;
(function(CodeActionKind$1) {
	CodeActionKind$1.Empty = "";
	CodeActionKind$1.QuickFix = "quickfix";
	CodeActionKind$1.Refactor = "refactor";
	CodeActionKind$1.RefactorExtract = "refactor.extract";
	CodeActionKind$1.RefactorInline = "refactor.inline";
	CodeActionKind$1.RefactorRewrite = "refactor.rewrite";
	CodeActionKind$1.Source = "source";
	CodeActionKind$1.SourceOrganizeImports = "source.organizeImports";
	CodeActionKind$1.SourceFixAll = "source.fixAll";
})(CodeActionKind || (CodeActionKind = {}));
var CodeActionTriggerKind;
(function(CodeActionTriggerKind$1) {
	CodeActionTriggerKind$1.Invoked = 1;
	CodeActionTriggerKind$1.Automatic = 2;
})(CodeActionTriggerKind || (CodeActionTriggerKind = {}));
var CodeActionContext;
(function(CodeActionContext$1) {
	function create(diagnostics, only, triggerKind) {
		let result = { diagnostics };
		if (only !== void 0 && only !== null) result.only = only;
		if (triggerKind !== void 0 && triggerKind !== null) result.triggerKind = triggerKind;
		return result;
	}
	CodeActionContext$1.create = create;
	function is(value) {
		let candidate = value;
		return Is.defined(candidate) && Is.typedArray(candidate.diagnostics, Diagnostic.is) && (candidate.only === void 0 || Is.typedArray(candidate.only, Is.string)) && (candidate.triggerKind === void 0 || candidate.triggerKind === CodeActionTriggerKind.Invoked || candidate.triggerKind === CodeActionTriggerKind.Automatic);
	}
	CodeActionContext$1.is = is;
})(CodeActionContext || (CodeActionContext = {}));
var CodeAction;
(function(CodeAction$1) {
	function create(title, kindOrCommandOrEdit, kind) {
		let result = { title };
		let checkKind = true;
		if (typeof kindOrCommandOrEdit === "string") {
			checkKind = false;
			result.kind = kindOrCommandOrEdit;
		} else if (Command.is(kindOrCommandOrEdit)) result.command = kindOrCommandOrEdit;
		else result.edit = kindOrCommandOrEdit;
		if (checkKind && kind !== void 0) result.kind = kind;
		return result;
	}
	CodeAction$1.create = create;
	function is(value) {
		let candidate = value;
		return candidate && Is.string(candidate.title) && (candidate.diagnostics === void 0 || Is.typedArray(candidate.diagnostics, Diagnostic.is)) && (candidate.kind === void 0 || Is.string(candidate.kind)) && (candidate.edit !== void 0 || candidate.command !== void 0) && (candidate.command === void 0 || Command.is(candidate.command)) && (candidate.isPreferred === void 0 || Is.boolean(candidate.isPreferred)) && (candidate.edit === void 0 || WorkspaceEdit.is(candidate.edit));
	}
	CodeAction$1.is = is;
})(CodeAction || (CodeAction = {}));
var CodeLens;
(function(CodeLens$1) {
	function create(range, data) {
		let result = { range };
		if (Is.defined(data)) result.data = data;
		return result;
	}
	CodeLens$1.create = create;
	function is(value) {
		let candidate = value;
		return Is.defined(candidate) && Range$1.is(candidate.range) && (Is.undefined(candidate.command) || Command.is(candidate.command));
	}
	CodeLens$1.is = is;
})(CodeLens || (CodeLens = {}));
var FormattingOptions;
(function(FormattingOptions$1) {
	function create(tabSize, insertSpaces) {
		return {
			tabSize,
			insertSpaces
		};
	}
	FormattingOptions$1.create = create;
	function is(value) {
		let candidate = value;
		return Is.defined(candidate) && Is.uinteger(candidate.tabSize) && Is.boolean(candidate.insertSpaces);
	}
	FormattingOptions$1.is = is;
})(FormattingOptions || (FormattingOptions = {}));
var DocumentLink;
(function(DocumentLink$1) {
	function create(range, target, data) {
		return {
			range,
			target,
			data
		};
	}
	DocumentLink$1.create = create;
	function is(value) {
		let candidate = value;
		return Is.defined(candidate) && Range$1.is(candidate.range) && (Is.undefined(candidate.target) || Is.string(candidate.target));
	}
	DocumentLink$1.is = is;
})(DocumentLink || (DocumentLink = {}));
var SelectionRange;
(function(SelectionRange$1) {
	function create(range, parent) {
		return {
			range,
			parent
		};
	}
	SelectionRange$1.create = create;
	function is(value) {
		let candidate = value;
		return Is.objectLiteral(candidate) && Range$1.is(candidate.range) && (candidate.parent === void 0 || SelectionRange$1.is(candidate.parent));
	}
	SelectionRange$1.is = is;
})(SelectionRange || (SelectionRange = {}));
var SemanticTokenTypes;
(function(SemanticTokenTypes$1) {
	SemanticTokenTypes$1["namespace"] = "namespace";
	SemanticTokenTypes$1["type"] = "type";
	SemanticTokenTypes$1["class"] = "class";
	SemanticTokenTypes$1["enum"] = "enum";
	SemanticTokenTypes$1["interface"] = "interface";
	SemanticTokenTypes$1["struct"] = "struct";
	SemanticTokenTypes$1["typeParameter"] = "typeParameter";
	SemanticTokenTypes$1["parameter"] = "parameter";
	SemanticTokenTypes$1["variable"] = "variable";
	SemanticTokenTypes$1["property"] = "property";
	SemanticTokenTypes$1["enumMember"] = "enumMember";
	SemanticTokenTypes$1["event"] = "event";
	SemanticTokenTypes$1["function"] = "function";
	SemanticTokenTypes$1["method"] = "method";
	SemanticTokenTypes$1["macro"] = "macro";
	SemanticTokenTypes$1["keyword"] = "keyword";
	SemanticTokenTypes$1["modifier"] = "modifier";
	SemanticTokenTypes$1["comment"] = "comment";
	SemanticTokenTypes$1["string"] = "string";
	SemanticTokenTypes$1["number"] = "number";
	SemanticTokenTypes$1["regexp"] = "regexp";
	SemanticTokenTypes$1["operator"] = "operator";
	SemanticTokenTypes$1["decorator"] = "decorator";
})(SemanticTokenTypes || (SemanticTokenTypes = {}));
var SemanticTokenModifiers;
(function(SemanticTokenModifiers$1) {
	SemanticTokenModifiers$1["declaration"] = "declaration";
	SemanticTokenModifiers$1["definition"] = "definition";
	SemanticTokenModifiers$1["readonly"] = "readonly";
	SemanticTokenModifiers$1["static"] = "static";
	SemanticTokenModifiers$1["deprecated"] = "deprecated";
	SemanticTokenModifiers$1["abstract"] = "abstract";
	SemanticTokenModifiers$1["async"] = "async";
	SemanticTokenModifiers$1["modification"] = "modification";
	SemanticTokenModifiers$1["documentation"] = "documentation";
	SemanticTokenModifiers$1["defaultLibrary"] = "defaultLibrary";
})(SemanticTokenModifiers || (SemanticTokenModifiers = {}));
var SemanticTokens;
(function(SemanticTokens$1) {
	function is(value) {
		const candidate = value;
		return Is.objectLiteral(candidate) && (candidate.resultId === void 0 || typeof candidate.resultId === "string") && Array.isArray(candidate.data) && (candidate.data.length === 0 || typeof candidate.data[0] === "number");
	}
	SemanticTokens$1.is = is;
})(SemanticTokens || (SemanticTokens = {}));
var InlineValueText;
(function(InlineValueText$1) {
	function create(range, text) {
		return {
			range,
			text
		};
	}
	InlineValueText$1.create = create;
	function is(value) {
		const candidate = value;
		return candidate !== void 0 && candidate !== null && Range$1.is(candidate.range) && Is.string(candidate.text);
	}
	InlineValueText$1.is = is;
})(InlineValueText || (InlineValueText = {}));
var InlineValueVariableLookup;
(function(InlineValueVariableLookup$1) {
	function create(range, variableName, caseSensitiveLookup) {
		return {
			range,
			variableName,
			caseSensitiveLookup
		};
	}
	InlineValueVariableLookup$1.create = create;
	function is(value) {
		const candidate = value;
		return candidate !== void 0 && candidate !== null && Range$1.is(candidate.range) && Is.boolean(candidate.caseSensitiveLookup) && (Is.string(candidate.variableName) || candidate.variableName === void 0);
	}
	InlineValueVariableLookup$1.is = is;
})(InlineValueVariableLookup || (InlineValueVariableLookup = {}));
var InlineValueEvaluatableExpression;
(function(InlineValueEvaluatableExpression$1) {
	function create(range, expression) {
		return {
			range,
			expression
		};
	}
	InlineValueEvaluatableExpression$1.create = create;
	function is(value) {
		const candidate = value;
		return candidate !== void 0 && candidate !== null && Range$1.is(candidate.range) && (Is.string(candidate.expression) || candidate.expression === void 0);
	}
	InlineValueEvaluatableExpression$1.is = is;
})(InlineValueEvaluatableExpression || (InlineValueEvaluatableExpression = {}));
var InlineValueContext;
(function(InlineValueContext$1) {
	function create(frameId, stoppedLocation) {
		return {
			frameId,
			stoppedLocation
		};
	}
	InlineValueContext$1.create = create;
	function is(value) {
		const candidate = value;
		return Is.defined(candidate) && Range$1.is(value.stoppedLocation);
	}
	InlineValueContext$1.is = is;
})(InlineValueContext || (InlineValueContext = {}));
var InlayHintKind;
(function(InlayHintKind$1) {
	InlayHintKind$1.Type = 1;
	InlayHintKind$1.Parameter = 2;
	function is(value) {
		return value === 1 || value === 2;
	}
	InlayHintKind$1.is = is;
})(InlayHintKind || (InlayHintKind = {}));
var InlayHintLabelPart;
(function(InlayHintLabelPart$1) {
	function create(value) {
		return { value };
	}
	InlayHintLabelPart$1.create = create;
	function is(value) {
		const candidate = value;
		return Is.objectLiteral(candidate) && (candidate.tooltip === void 0 || Is.string(candidate.tooltip) || MarkupContent.is(candidate.tooltip)) && (candidate.location === void 0 || Location.is(candidate.location)) && (candidate.command === void 0 || Command.is(candidate.command));
	}
	InlayHintLabelPart$1.is = is;
})(InlayHintLabelPart || (InlayHintLabelPart = {}));
var InlayHint;
(function(InlayHint$1) {
	function create(position, label, kind) {
		const result = {
			position,
			label
		};
		if (kind !== void 0) result.kind = kind;
		return result;
	}
	InlayHint$1.create = create;
	function is(value) {
		const candidate = value;
		return Is.objectLiteral(candidate) && Position.is(candidate.position) && (Is.string(candidate.label) || Is.typedArray(candidate.label, InlayHintLabelPart.is)) && (candidate.kind === void 0 || InlayHintKind.is(candidate.kind)) && candidate.textEdits === void 0 || Is.typedArray(candidate.textEdits, TextEdit.is) && (candidate.tooltip === void 0 || Is.string(candidate.tooltip) || MarkupContent.is(candidate.tooltip)) && (candidate.paddingLeft === void 0 || Is.boolean(candidate.paddingLeft)) && (candidate.paddingRight === void 0 || Is.boolean(candidate.paddingRight));
	}
	InlayHint$1.is = is;
})(InlayHint || (InlayHint = {}));
var StringValue;
(function(StringValue$1) {
	function createSnippet(value) {
		return {
			kind: "snippet",
			value
		};
	}
	StringValue$1.createSnippet = createSnippet;
})(StringValue || (StringValue = {}));
var InlineCompletionItem;
(function(InlineCompletionItem$1) {
	function create(insertText, filterText, range, command) {
		return {
			insertText,
			filterText,
			range,
			command
		};
	}
	InlineCompletionItem$1.create = create;
})(InlineCompletionItem || (InlineCompletionItem = {}));
var InlineCompletionList;
(function(InlineCompletionList$1) {
	function create(items) {
		return { items };
	}
	InlineCompletionList$1.create = create;
})(InlineCompletionList || (InlineCompletionList = {}));
var InlineCompletionTriggerKind;
(function(InlineCompletionTriggerKind$1) {
	InlineCompletionTriggerKind$1.Invoked = 0;
	InlineCompletionTriggerKind$1.Automatic = 1;
})(InlineCompletionTriggerKind || (InlineCompletionTriggerKind = {}));
var SelectedCompletionInfo;
(function(SelectedCompletionInfo$1) {
	function create(range, text) {
		return {
			range,
			text
		};
	}
	SelectedCompletionInfo$1.create = create;
})(SelectedCompletionInfo || (SelectedCompletionInfo = {}));
var InlineCompletionContext;
(function(InlineCompletionContext$1) {
	function create(triggerKind, selectedCompletionInfo) {
		return {
			triggerKind,
			selectedCompletionInfo
		};
	}
	InlineCompletionContext$1.create = create;
})(InlineCompletionContext || (InlineCompletionContext = {}));
var WorkspaceFolder;
(function(WorkspaceFolder$1) {
	function is(value) {
		const candidate = value;
		return Is.objectLiteral(candidate) && URI.is(candidate.uri) && Is.string(candidate.name);
	}
	WorkspaceFolder$1.is = is;
})(WorkspaceFolder || (WorkspaceFolder = {}));
var TextDocument;
(function(TextDocument$1) {
	function create(uri, languageId, version, content) {
		return new FullTextDocument(uri, languageId, version, content);
	}
	TextDocument$1.create = create;
	function is(value) {
		let candidate = value;
		return Is.defined(candidate) && Is.string(candidate.uri) && (Is.undefined(candidate.languageId) || Is.string(candidate.languageId)) && Is.uinteger(candidate.lineCount) && Is.func(candidate.getText) && Is.func(candidate.positionAt) && Is.func(candidate.offsetAt) ? true : false;
	}
	TextDocument$1.is = is;
	function applyEdits(document, edits) {
		let text = document.getText();
		let sortedEdits = mergeSort(edits, (a, b) => {
			let diff = a.range.start.line - b.range.start.line;
			if (diff === 0) return a.range.start.character - b.range.start.character;
			return diff;
		});
		let lastModifiedOffset = text.length;
		for (let i = sortedEdits.length - 1; i >= 0; i--) {
			let e = sortedEdits[i];
			let startOffset = document.offsetAt(e.range.start);
			let endOffset = document.offsetAt(e.range.end);
			if (endOffset <= lastModifiedOffset) text = text.substring(0, startOffset) + e.newText + text.substring(endOffset, text.length);
			else throw new Error("Overlapping edit");
			lastModifiedOffset = startOffset;
		}
		return text;
	}
	TextDocument$1.applyEdits = applyEdits;
	function mergeSort(data, compare) {
		if (data.length <= 1) return data;
		const p = data.length / 2 | 0;
		const left = data.slice(0, p);
		const right = data.slice(p);
		mergeSort(left, compare);
		mergeSort(right, compare);
		let leftIdx = 0;
		let rightIdx = 0;
		let i = 0;
		while (leftIdx < left.length && rightIdx < right.length) if (compare(left[leftIdx], right[rightIdx]) <= 0) data[i++] = left[leftIdx++];
		else data[i++] = right[rightIdx++];
		while (leftIdx < left.length) data[i++] = left[leftIdx++];
		while (rightIdx < right.length) data[i++] = right[rightIdx++];
		return data;
	}
})(TextDocument || (TextDocument = {}));
var FullTextDocument = class {
	constructor(uri, languageId, version, content) {
		this._uri = uri;
		this._languageId = languageId;
		this._version = version;
		this._content = content;
		this._lineOffsets = void 0;
	}
	get uri() {
		return this._uri;
	}
	get languageId() {
		return this._languageId;
	}
	get version() {
		return this._version;
	}
	getText(range) {
		if (range) {
			let start = this.offsetAt(range.start);
			let end = this.offsetAt(range.end);
			return this._content.substring(start, end);
		}
		return this._content;
	}
	update(event, version) {
		this._content = event.text;
		this._version = version;
		this._lineOffsets = void 0;
	}
	getLineOffsets() {
		if (this._lineOffsets === void 0) {
			let lineOffsets = [];
			let text = this._content;
			let isLineStart = true;
			for (let i = 0; i < text.length; i++) {
				if (isLineStart) {
					lineOffsets.push(i);
					isLineStart = false;
				}
				let ch = text.charAt(i);
				isLineStart = ch === "\r" || ch === "\n";
				if (ch === "\r" && i + 1 < text.length && text.charAt(i + 1) === "\n") i++;
			}
			if (isLineStart && text.length > 0) lineOffsets.push(text.length);
			this._lineOffsets = lineOffsets;
		}
		return this._lineOffsets;
	}
	positionAt(offset) {
		offset = Math.max(Math.min(offset, this._content.length), 0);
		let lineOffsets = this.getLineOffsets();
		let low = 0, high = lineOffsets.length;
		if (high === 0) return Position.create(0, offset);
		while (low < high) {
			let mid = Math.floor((low + high) / 2);
			if (lineOffsets[mid] > offset) high = mid;
			else low = mid + 1;
		}
		let line = low - 1;
		return Position.create(line, offset - lineOffsets[line]);
	}
	offsetAt(position) {
		let lineOffsets = this.getLineOffsets();
		if (position.line >= lineOffsets.length) return this._content.length;
		else if (position.line < 0) return 0;
		let lineOffset = lineOffsets[position.line];
		let nextLineOffset = position.line + 1 < lineOffsets.length ? lineOffsets[position.line + 1] : this._content.length;
		return Math.max(Math.min(lineOffset + position.character, nextLineOffset), lineOffset);
	}
	get lineCount() {
		return this.getLineOffsets().length;
	}
};
var Is;
(function(Is$1) {
	const toString = Object.prototype.toString;
	function defined(value) {
		return typeof value !== "undefined";
	}
	Is$1.defined = defined;
	function undefined$1(value) {
		return typeof value === "undefined";
	}
	Is$1.undefined = undefined$1;
	function boolean(value) {
		return value === true || value === false;
	}
	Is$1.boolean = boolean;
	function string(value) {
		return toString.call(value) === "[object String]";
	}
	Is$1.string = string;
	function number(value) {
		return toString.call(value) === "[object Number]";
	}
	Is$1.number = number;
	function numberRange(value, min, max) {
		return toString.call(value) === "[object Number]" && min <= value && value <= max;
	}
	Is$1.numberRange = numberRange;
	function integer$1(value) {
		return toString.call(value) === "[object Number]" && -2147483648 <= value && value <= 2147483647;
	}
	Is$1.integer = integer$1;
	function uinteger$1(value) {
		return toString.call(value) === "[object Number]" && 0 <= value && value <= 2147483647;
	}
	Is$1.uinteger = uinteger$1;
	function func(value) {
		return toString.call(value) === "[object Function]";
	}
	Is$1.func = func;
	function objectLiteral(value) {
		return value !== null && typeof value === "object";
	}
	Is$1.objectLiteral = objectLiteral;
	function typedArray(value, check) {
		return Array.isArray(value) && value.every(check);
	}
	Is$1.typedArray = typedArray;
})(Is || (Is = {}));
var DiagnosticsAdapter = class {
	constructor(_languageId, _worker, configChangeEvent) {
		this._languageId = _languageId;
		this._worker = _worker;
		this._disposables = [];
		this._listener = /* @__PURE__ */ Object.create(null);
		const onModelAdd = (model) => {
			let modeId = model.getLanguageId();
			if (modeId !== this._languageId) return;
			let handle;
			this._listener[model.uri.toString()] = model.onDidChangeContent(() => {
				window.clearTimeout(handle);
				handle = window.setTimeout(() => this._doValidate(model.uri, modeId), 500);
			});
			this._doValidate(model.uri, modeId);
		};
		const onModelRemoved = (model) => {
			editor.setModelMarkers(model, this._languageId, []);
			let uriStr = model.uri.toString();
			let listener = this._listener[uriStr];
			if (listener) {
				listener.dispose();
				delete this._listener[uriStr];
			}
		};
		this._disposables.push(editor.onDidCreateModel(onModelAdd));
		this._disposables.push(editor.onWillDisposeModel(onModelRemoved));
		this._disposables.push(editor.onDidChangeModelLanguage((event) => {
			onModelRemoved(event.model);
			onModelAdd(event.model);
		}));
		this._disposables.push(configChangeEvent((_) => {
			editor.getModels().forEach((model) => {
				if (model.getLanguageId() === this._languageId) {
					onModelRemoved(model);
					onModelAdd(model);
				}
			});
		}));
		this._disposables.push({ dispose: () => {
			editor.getModels().forEach(onModelRemoved);
			for (let key in this._listener) this._listener[key].dispose();
		} });
		editor.getModels().forEach(onModelAdd);
	}
	dispose() {
		this._disposables.forEach((d) => d && d.dispose());
		this._disposables.length = 0;
	}
	_doValidate(resource, languageId) {
		this._worker(resource).then((worker) => {
			return worker.doValidation(resource.toString());
		}).then((diagnostics) => {
			const markers = diagnostics.map((d) => toDiagnostics(resource, d));
			let model = editor.getModel(resource);
			if (model && model.getLanguageId() === languageId) editor.setModelMarkers(model, languageId, markers);
		}).then(void 0, (err) => {
			console.error(err);
		});
	}
};
function toSeverity(lsSeverity) {
	switch (lsSeverity) {
		case DiagnosticSeverity.Error: return MarkerSeverity.Error;
		case DiagnosticSeverity.Warning: return MarkerSeverity.Warning;
		case DiagnosticSeverity.Information: return MarkerSeverity.Info;
		case DiagnosticSeverity.Hint: return MarkerSeverity.Hint;
		default: return MarkerSeverity.Info;
	}
}
function toDiagnostics(resource, diag) {
	let code = typeof diag.code === "number" ? String(diag.code) : diag.code;
	return {
		severity: toSeverity(diag.severity),
		startLineNumber: diag.range.start.line + 1,
		startColumn: diag.range.start.character + 1,
		endLineNumber: diag.range.end.line + 1,
		endColumn: diag.range.end.character + 1,
		message: diag.message,
		code,
		source: diag.source
	};
}
var CompletionAdapter = class {
	constructor(_worker, _triggerCharacters) {
		this._worker = _worker;
		this._triggerCharacters = _triggerCharacters;
	}
	get triggerCharacters() {
		return this._triggerCharacters;
	}
	provideCompletionItems(model, position, context, token) {
		const resource = model.uri;
		return this._worker(resource).then((worker) => {
			return worker.doComplete(resource.toString(), fromPosition(position));
		}).then((info) => {
			if (!info) return;
			const wordInfo = model.getWordUntilPosition(position);
			const wordRange = new Range(position.lineNumber, wordInfo.startColumn, position.lineNumber, wordInfo.endColumn);
			const items = info.items.map((entry) => {
				const item = {
					label: entry.label,
					insertText: entry.insertText || entry.label,
					sortText: entry.sortText,
					filterText: entry.filterText,
					documentation: entry.documentation,
					detail: entry.detail,
					command: toCommand(entry.command),
					range: wordRange,
					kind: toCompletionItemKind(entry.kind)
				};
				if (entry.textEdit) {
					if (isInsertReplaceEdit(entry.textEdit)) item.range = {
						insert: toRange(entry.textEdit.insert),
						replace: toRange(entry.textEdit.replace)
					};
					else item.range = toRange(entry.textEdit.range);
					item.insertText = entry.textEdit.newText;
				}
				if (entry.additionalTextEdits) item.additionalTextEdits = entry.additionalTextEdits.map(toTextEdit);
				if (entry.insertTextFormat === InsertTextFormat.Snippet) item.insertTextRules = languages.CompletionItemInsertTextRule.InsertAsSnippet;
				return item;
			});
			return {
				isIncomplete: info.isIncomplete,
				suggestions: items
			};
		});
	}
};
function fromPosition(position) {
	if (!position) return;
	return {
		character: position.column - 1,
		line: position.lineNumber - 1
	};
}
function fromRange(range) {
	if (!range) return;
	return {
		start: {
			line: range.startLineNumber - 1,
			character: range.startColumn - 1
		},
		end: {
			line: range.endLineNumber - 1,
			character: range.endColumn - 1
		}
	};
}
function toRange(range) {
	if (!range) return;
	return new Range(range.start.line + 1, range.start.character + 1, range.end.line + 1, range.end.character + 1);
}
function isInsertReplaceEdit(edit) {
	return typeof edit.insert !== "undefined" && typeof edit.replace !== "undefined";
}
function toCompletionItemKind(kind) {
	const mItemKind = languages.CompletionItemKind;
	switch (kind) {
		case CompletionItemKind.Text: return mItemKind.Text;
		case CompletionItemKind.Method: return mItemKind.Method;
		case CompletionItemKind.Function: return mItemKind.Function;
		case CompletionItemKind.Constructor: return mItemKind.Constructor;
		case CompletionItemKind.Field: return mItemKind.Field;
		case CompletionItemKind.Variable: return mItemKind.Variable;
		case CompletionItemKind.Class: return mItemKind.Class;
		case CompletionItemKind.Interface: return mItemKind.Interface;
		case CompletionItemKind.Module: return mItemKind.Module;
		case CompletionItemKind.Property: return mItemKind.Property;
		case CompletionItemKind.Unit: return mItemKind.Unit;
		case CompletionItemKind.Value: return mItemKind.Value;
		case CompletionItemKind.Enum: return mItemKind.Enum;
		case CompletionItemKind.Keyword: return mItemKind.Keyword;
		case CompletionItemKind.Snippet: return mItemKind.Snippet;
		case CompletionItemKind.Color: return mItemKind.Color;
		case CompletionItemKind.File: return mItemKind.File;
		case CompletionItemKind.Reference: return mItemKind.Reference;
	}
	return mItemKind.Property;
}
function toTextEdit(textEdit) {
	if (!textEdit) return;
	return {
		range: toRange(textEdit.range),
		text: textEdit.newText
	};
}
function toCommand(c) {
	return c && c.command === "editor.action.triggerSuggest" ? {
		id: c.command,
		title: c.title,
		arguments: c.arguments
	} : void 0;
}
var HoverAdapter = class {
	constructor(_worker) {
		this._worker = _worker;
	}
	provideHover(model, position, token) {
		let resource = model.uri;
		return this._worker(resource).then((worker) => {
			return worker.doHover(resource.toString(), fromPosition(position));
		}).then((info) => {
			if (!info) return;
			return {
				range: toRange(info.range),
				contents: toMarkedStringArray(info.contents)
			};
		});
	}
};
function isMarkupContent(thing) {
	return thing && typeof thing === "object" && typeof thing.kind === "string";
}
function toMarkdownString(entry) {
	if (typeof entry === "string") return { value: entry };
	if (isMarkupContent(entry)) {
		if (entry.kind === "plaintext") return { value: entry.value.replace(/[\\`*_{}[\]()#+\-.!]/g, "\\$&") };
		return { value: entry.value };
	}
	return { value: "```" + entry.language + "\n" + entry.value + "\n```\n" };
}
function toMarkedStringArray(contents) {
	if (!contents) return;
	if (Array.isArray(contents)) return contents.map(toMarkdownString);
	return [toMarkdownString(contents)];
}
var DocumentHighlightAdapter = class {
	constructor(_worker) {
		this._worker = _worker;
	}
	provideDocumentHighlights(model, position, token) {
		const resource = model.uri;
		return this._worker(resource).then((worker) => worker.findDocumentHighlights(resource.toString(), fromPosition(position))).then((entries) => {
			if (!entries) return;
			return entries.map((entry) => {
				return {
					range: toRange(entry.range),
					kind: toDocumentHighlightKind(entry.kind)
				};
			});
		});
	}
};
function toDocumentHighlightKind(kind) {
	switch (kind) {
		case DocumentHighlightKind.Read: return languages.DocumentHighlightKind.Read;
		case DocumentHighlightKind.Write: return languages.DocumentHighlightKind.Write;
		case DocumentHighlightKind.Text: return languages.DocumentHighlightKind.Text;
	}
	return languages.DocumentHighlightKind.Text;
}
var DefinitionAdapter = class {
	constructor(_worker) {
		this._worker = _worker;
	}
	provideDefinition(model, position, token) {
		const resource = model.uri;
		return this._worker(resource).then((worker) => {
			return worker.findDefinition(resource.toString(), fromPosition(position));
		}).then((definition) => {
			if (!definition) return;
			return [toLocation(definition)];
		});
	}
};
function toLocation(location) {
	return {
		uri: Uri.parse(location.uri),
		range: toRange(location.range)
	};
}
var ReferenceAdapter = class {
	constructor(_worker) {
		this._worker = _worker;
	}
	provideReferences(model, position, context, token) {
		const resource = model.uri;
		return this._worker(resource).then((worker) => {
			return worker.findReferences(resource.toString(), fromPosition(position));
		}).then((entries) => {
			if (!entries) return;
			return entries.map(toLocation);
		});
	}
};
var RenameAdapter = class {
	constructor(_worker) {
		this._worker = _worker;
	}
	provideRenameEdits(model, position, newName, token) {
		const resource = model.uri;
		return this._worker(resource).then((worker) => {
			return worker.doRename(resource.toString(), fromPosition(position), newName);
		}).then((edit) => {
			return toWorkspaceEdit(edit);
		});
	}
};
function toWorkspaceEdit(edit) {
	if (!edit || !edit.changes) return;
	let resourceEdits = [];
	for (let uri in edit.changes) {
		const _uri = Uri.parse(uri);
		for (let e of edit.changes[uri]) resourceEdits.push({
			resource: _uri,
			versionId: void 0,
			textEdit: {
				range: toRange(e.range),
				text: e.newText
			}
		});
	}
	return { edits: resourceEdits };
}
var DocumentSymbolAdapter = class {
	constructor(_worker) {
		this._worker = _worker;
	}
	provideDocumentSymbols(model, token) {
		const resource = model.uri;
		return this._worker(resource).then((worker) => worker.findDocumentSymbols(resource.toString())).then((items) => {
			if (!items) return;
			return items.map((item) => {
				if (isDocumentSymbol(item)) return toDocumentSymbol(item);
				return {
					name: item.name,
					detail: "",
					containerName: item.containerName,
					kind: toSymbolKind(item.kind),
					range: toRange(item.location.range),
					selectionRange: toRange(item.location.range),
					tags: []
				};
			});
		});
	}
};
function isDocumentSymbol(symbol) {
	return "children" in symbol;
}
function toDocumentSymbol(symbol) {
	return {
		name: symbol.name,
		detail: symbol.detail ?? "",
		kind: toSymbolKind(symbol.kind),
		range: toRange(symbol.range),
		selectionRange: toRange(symbol.selectionRange),
		tags: symbol.tags ?? [],
		children: (symbol.children ?? []).map((item) => toDocumentSymbol(item))
	};
}
function toSymbolKind(kind) {
	let mKind = languages.SymbolKind;
	switch (kind) {
		case SymbolKind.File: return mKind.File;
		case SymbolKind.Module: return mKind.Module;
		case SymbolKind.Namespace: return mKind.Namespace;
		case SymbolKind.Package: return mKind.Package;
		case SymbolKind.Class: return mKind.Class;
		case SymbolKind.Method: return mKind.Method;
		case SymbolKind.Property: return mKind.Property;
		case SymbolKind.Field: return mKind.Field;
		case SymbolKind.Constructor: return mKind.Constructor;
		case SymbolKind.Enum: return mKind.Enum;
		case SymbolKind.Interface: return mKind.Interface;
		case SymbolKind.Function: return mKind.Function;
		case SymbolKind.Variable: return mKind.Variable;
		case SymbolKind.Constant: return mKind.Constant;
		case SymbolKind.String: return mKind.String;
		case SymbolKind.Number: return mKind.Number;
		case SymbolKind.Boolean: return mKind.Boolean;
		case SymbolKind.Array: return mKind.Array;
	}
	return mKind.Function;
}
var DocumentLinkAdapter = class {
	constructor(_worker) {
		this._worker = _worker;
	}
	provideLinks(model, token) {
		const resource = model.uri;
		return this._worker(resource).then((worker) => worker.findDocumentLinks(resource.toString())).then((items) => {
			if (!items) return;
			return { links: items.map((item) => ({
				range: toRange(item.range),
				url: item.target
			})) };
		});
	}
};
var DocumentFormattingEditProvider = class {
	constructor(_worker) {
		this._worker = _worker;
	}
	provideDocumentFormattingEdits(model, options, token) {
		const resource = model.uri;
		return this._worker(resource).then((worker) => {
			return worker.format(resource.toString(), null, fromFormattingOptions(options)).then((edits) => {
				if (!edits || edits.length === 0) return;
				return edits.map(toTextEdit);
			});
		});
	}
};
var DocumentRangeFormattingEditProvider = class {
	constructor(_worker) {
		this._worker = _worker;
		this.canFormatMultipleRanges = false;
	}
	provideDocumentRangeFormattingEdits(model, range, options, token) {
		const resource = model.uri;
		return this._worker(resource).then((worker) => {
			return worker.format(resource.toString(), fromRange(range), fromFormattingOptions(options)).then((edits) => {
				if (!edits || edits.length === 0) return;
				return edits.map(toTextEdit);
			});
		});
	}
};
function fromFormattingOptions(options) {
	return {
		tabSize: options.tabSize,
		insertSpaces: options.insertSpaces
	};
}
var DocumentColorAdapter = class {
	constructor(_worker) {
		this._worker = _worker;
	}
	provideDocumentColors(model, token) {
		const resource = model.uri;
		return this._worker(resource).then((worker) => worker.findDocumentColors(resource.toString())).then((infos) => {
			if (!infos) return;
			return infos.map((item) => ({
				color: item.color,
				range: toRange(item.range)
			}));
		});
	}
	provideColorPresentations(model, info, token) {
		const resource = model.uri;
		return this._worker(resource).then((worker) => worker.getColorPresentations(resource.toString(), info.color, fromRange(info.range))).then((presentations) => {
			if (!presentations) return;
			return presentations.map((presentation) => {
				let item = { label: presentation.label };
				if (presentation.textEdit) item.textEdit = toTextEdit(presentation.textEdit);
				if (presentation.additionalTextEdits) item.additionalTextEdits = presentation.additionalTextEdits.map(toTextEdit);
				return item;
			});
		});
	}
};
var FoldingRangeAdapter = class {
	constructor(_worker) {
		this._worker = _worker;
	}
	provideFoldingRanges(model, context, token) {
		const resource = model.uri;
		return this._worker(resource).then((worker) => worker.getFoldingRanges(resource.toString(), context)).then((ranges) => {
			if (!ranges) return;
			return ranges.map((range) => {
				const result = {
					start: range.startLine + 1,
					end: range.endLine + 1
				};
				if (typeof range.kind !== "undefined") result.kind = toFoldingRangeKind(range.kind);
				return result;
			});
		});
	}
};
function toFoldingRangeKind(kind) {
	switch (kind) {
		case FoldingRangeKind.Comment: return languages.FoldingRangeKind.Comment;
		case FoldingRangeKind.Imports: return languages.FoldingRangeKind.Imports;
		case FoldingRangeKind.Region: return languages.FoldingRangeKind.Region;
	}
}
var SelectionRangeAdapter = class {
	constructor(_worker) {
		this._worker = _worker;
	}
	provideSelectionRanges(model, positions, token) {
		const resource = model.uri;
		return this._worker(resource).then((worker) => worker.getSelectionRanges(resource.toString(), positions.map(fromPosition))).then((selectionRanges) => {
			if (!selectionRanges) return;
			return selectionRanges.map((selectionRange) => {
				const result = [];
				while (selectionRange) {
					result.push({ range: toRange(selectionRange.range) });
					selectionRange = selectionRange.parent;
				}
				return result;
			});
		});
	}
};
export { toRange as _, DocumentFormattingEditProvider as a, DocumentRangeFormattingEditProvider as c, HoverAdapter as d, ReferenceAdapter as f, fromRange as g, fromPosition as h, DocumentColorAdapter as i, DocumentSymbolAdapter as l, SelectionRangeAdapter as m, DefinitionAdapter as n, DocumentHighlightAdapter as o, RenameAdapter as p, DiagnosticsAdapter as r, DocumentLinkAdapter as s, CompletionAdapter as t, FoldingRangeAdapter as u, toTextEdit as v };
