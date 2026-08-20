import "./purify.es-C_rn83UJ.js";
import "./chunk-KEIR6QF5-XwhODICK.js";
import "./chunk-MOZMSUNE-CcKYuWnm.js";
import "./chunk-OSBZ3O6U-CH3rGoG-.js";
import "./chunk-5JV3BV7I-1IAmjKDB.js";
import "./chunk-CYSBUYHQ-CB-FGyEu.js";
import "./chunk-BIQX33UG-CAkRtdoy.js";
import "./chunk-EMLP6XTP-BMukZhRX.js";
import "./chunk-YOTPTUD7-C7xEeX8u.js";
import "./chunk-QBLGF6JB-DnxjFx6i.js";
import "./chunk-5TONJI2A-SKlxK3bm.js";
import "./chunk-5HE753X5-BURdYK09.js";
import "./chunk-U6XO7XAA-D3PEC3N4.js";
import "./chunk-JG7HCLWE-B91zDApt.js";
import "./chunk-CQNSW5MT-8iQCDHSN.js";
import "./chunk-R7FJI6CG-gQOQs0xQ.js";
import "./chunk-5FCAYU7R-BfEra_5g.js";
import { n as __name } from "./chunk-Y2CYZVJY-DUyCWguD.js";
import { m as log } from "./src-BFzutHoD.js";
import { H as setAccDescription, K as setDiagramTitle, U as setAccTitle, a as clear, c as configureSvgSize, f as defaultConfig_default, v as getAccDescription, w as getDiagramTitle, x as getConfig2, y as getAccTitle } from "./chunk-I66GZJ75-7VFc6IgS.js";
import { t as ordinal } from "./ordinal-ij_xRoN7.js";
import { n as constant_default } from "./path-qJns2Yva.js";
import { m as tau } from "./dist-D77b-jAv.js";
import { t as arc_default } from "./arc-LnbzeX9C.js";
import { t as array_default } from "./array-Bg23kMkO.js";
import { a as cleanAndMerge, m as parseFontSize } from "./chunk-NSK5VX7P-s-SUMTDp.js";
import { t as selectSvgElement } from "./chunk-3NCLNEKW-Cn4Yk-Ir.js";
import { t as populateCommonDb } from "./chunk-JWPE2WC7-Cm_QrJjD.js";
import { n as parse } from "./mermaid-parser.core-CWzMSSqh.js";
function descending_default(a, b) {
	return b < a ? -1 : b > a ? 1 : b >= a ? 0 : NaN;
}
function identity_default(d) {
	return d;
}
function pie_default() {
	var value = identity_default, sortValues = descending_default, sort = null, startAngle = constant_default(0), endAngle = constant_default(tau), padAngle = constant_default(0);
	function pie(data) {
		var i, n = (data = array_default(data)).length, j, k, sum = 0, index = new Array(n), arcs = new Array(n), a0 = +startAngle.apply(this, arguments), da = Math.min(tau, Math.max(-tau, endAngle.apply(this, arguments) - a0)), a1, p = Math.min(Math.abs(da) / n, padAngle.apply(this, arguments)), pa = p * (da < 0 ? -1 : 1), v;
		for (i = 0; i < n; ++i) if ((v = arcs[index[i] = i] = +value(data[i], i, data)) > 0) sum += v;
		if (sortValues != null) index.sort(function(i$1, j$1) {
			return sortValues(arcs[i$1], arcs[j$1]);
		});
		else if (sort != null) index.sort(function(i$1, j$1) {
			return sort(data[i$1], data[j$1]);
		});
		for (i = 0, k = sum ? (da - n * pa) / sum : 0; i < n; ++i, a0 = a1) j = index[i], v = arcs[j], a1 = a0 + (v > 0 ? v * k : 0) + pa, arcs[j] = {
			data: data[j],
			index: i,
			value: v,
			startAngle: a0,
			endAngle: a1,
			padAngle: p
		};
		return arcs;
	}
	pie.value = function(_) {
		return arguments.length ? (value = typeof _ === "function" ? _ : constant_default(+_), pie) : value;
	};
	pie.sortValues = function(_) {
		return arguments.length ? (sortValues = _, sort = null, pie) : sortValues;
	};
	pie.sort = function(_) {
		return arguments.length ? (sort = _, sortValues = null, pie) : sort;
	};
	pie.startAngle = function(_) {
		return arguments.length ? (startAngle = typeof _ === "function" ? _ : constant_default(+_), pie) : startAngle;
	};
	pie.endAngle = function(_) {
		return arguments.length ? (endAngle = typeof _ === "function" ? _ : constant_default(+_), pie) : endAngle;
	};
	pie.padAngle = function(_) {
		return arguments.length ? (padAngle = typeof _ === "function" ? _ : constant_default(+_), pie) : padAngle;
	};
	return pie;
}
var DEFAULT_PIE_CONFIG = defaultConfig_default.pie;
var DEFAULT_PIE_DB = {
	sections: /* @__PURE__ */ new Map(),
	showData: false,
	config: DEFAULT_PIE_CONFIG
};
var sections = DEFAULT_PIE_DB.sections;
var showData = DEFAULT_PIE_DB.showData;
var config = structuredClone(DEFAULT_PIE_CONFIG);
var db = {
	getConfig: /* @__PURE__ */ __name(() => structuredClone(config), "getConfig"),
	clear: /* @__PURE__ */ __name(() => {
		sections = /* @__PURE__ */ new Map();
		showData = DEFAULT_PIE_DB.showData;
		clear();
	}, "clear"),
	setDiagramTitle,
	getDiagramTitle,
	setAccTitle,
	getAccTitle,
	setAccDescription,
	getAccDescription,
	addSection: /* @__PURE__ */ __name(({ label, value }) => {
		if (value < 0) throw new Error(`"${label}" has invalid value: ${value}. Negative values are not allowed in pie charts. All slice values must be >= 0.`);
		if (!sections.has(label)) {
			sections.set(label, value);
			log.debug(`added new section: ${label}, with value: ${value}`);
		}
	}, "addSection"),
	getSections: /* @__PURE__ */ __name(() => sections, "getSections"),
	setShowData: /* @__PURE__ */ __name((toggle) => {
		showData = toggle;
	}, "setShowData"),
	getShowData: /* @__PURE__ */ __name(() => showData, "getShowData")
};
var populateDb = /* @__PURE__ */ __name((ast, db2) => {
	populateCommonDb(ast, db2);
	db2.setShowData(ast.showData);
	ast.sections.map(db2.addSection);
}, "populateDb");
var parser = { parse: /* @__PURE__ */ __name(async (input) => {
	const ast = await parse("pie", input);
	log.debug(ast);
	populateDb(ast, db);
}, "parse") };
var pieStyles_default = /* @__PURE__ */ __name((options) => `
  .pieCircle{
    stroke: ${options.pieStrokeColor};
    stroke-width : ${options.pieStrokeWidth};
    opacity : ${options.pieOpacity};
  }
  .pieCircle.highlighted{
    scale: 1.05;
    opacity: 1;
  }
  .pieCircle.highlightedOnHover:hover{
    transition-duration: 250ms;
    scale: 1.05;
    opacity: 1;
  }
  .pieOuterCircle{
    stroke: ${options.pieOuterStrokeColor};
    stroke-width: ${options.pieOuterStrokeWidth};
    fill: none;
  }
  .pieTitleText {
    text-anchor: middle;
    font-size: ${options.pieTitleTextSize};
    fill: ${options.pieTitleTextColor};
    font-family: ${options.fontFamily};
  }
  .slice {
    font-family: ${options.fontFamily};
    fill: ${options.pieSectionTextColor};
    font-size:${options.pieSectionTextSize};
    // fill: white;
  }
  .legend text {
    fill: ${options.pieLegendTextColor};
    font-family: ${options.fontFamily};
    font-size: ${options.pieLegendTextSize};
  }
`, "getStyles");
var createPieArcs = /* @__PURE__ */ __name((sections2) => {
	const sum = [...sections2.values()].reduce((acc, val) => acc + val, 0);
	const pieData = [...sections2.entries()].map(([label, value]) => ({
		label,
		value
	})).filter((d) => d.value / sum * 100 >= 1);
	return pie_default().value((d) => d.value).sort(null)(pieData);
}, "createPieArcs");
var diagram = {
	parser,
	db,
	renderer: { draw: /* @__PURE__ */ __name((text, id, _version, diagObj) => {
		log.debug("rendering pie chart\n" + text);
		const db2 = diagObj.db;
		const globalConfig = getConfig2();
		const pieConfig = cleanAndMerge(db2.getConfig(), globalConfig.pie);
		const MARGIN = 40;
		const LEGEND_RECT_SIZE = 18;
		const LEGEND_SPACING = 4;
		const height = 450;
		const pieWidth = height;
		const svg = selectSvgElement(id);
		const group = svg.append("g");
		group.attr("transform", "translate(" + pieWidth / 2 + "," + height / 2 + ")");
		const { themeVariables } = globalConfig;
		let [outerStrokeWidth] = parseFontSize(themeVariables.pieOuterStrokeWidth);
		outerStrokeWidth ?? (outerStrokeWidth = 2);
		const legendPosition = pieConfig.legendPosition;
		const textPosition = pieConfig.textPosition;
		const innerHole = pieConfig.donutHole > 0 && pieConfig.donutHole <= .9 ? pieConfig.donutHole : 0;
		const radius = Math.min(pieWidth, height) / 2 - MARGIN;
		const arcGenerator = arc_default().innerRadius(innerHole * radius).outerRadius(radius);
		const labelArcGenerator = arc_default().innerRadius(radius * textPosition).outerRadius(radius * textPosition);
		const pie = group.append("g");
		pie.append("circle").attr("cx", 0).attr("cy", 0).attr("r", radius + outerStrokeWidth / 2).attr("class", "pieOuterCircle");
		const sections2 = db2.getSections();
		const arcs = createPieArcs(sections2);
		const myGeneratedColors = [
			themeVariables.pie1,
			themeVariables.pie2,
			themeVariables.pie3,
			themeVariables.pie4,
			themeVariables.pie5,
			themeVariables.pie6,
			themeVariables.pie7,
			themeVariables.pie8,
			themeVariables.pie9,
			themeVariables.pie10,
			themeVariables.pie11,
			themeVariables.pie12
		];
		let sum = 0;
		sections2.forEach((section) => {
			sum += section;
		});
		const filteredArcs = arcs.filter((datum) => (datum.data.value / sum * 100).toFixed(0) !== "0");
		const color = ordinal(myGeneratedColors).domain([...sections2.keys()]);
		pie.selectAll("mySlices").data(filteredArcs).enter().append("path").attr("d", arcGenerator).attr("fill", (datum) => {
			return color(datum.data.label);
		}).attr("class", (datum) => {
			let className = "pieCircle";
			if (pieConfig.highlightSlice === "hover") className += " highlightedOnHover";
			else if (pieConfig.highlightSlice === datum.data.label) className += " highlighted";
			return className;
		});
		pie.selectAll("mySlices").data(filteredArcs).enter().append("text").text((datum) => {
			return (datum.data.value / sum * 100).toFixed(0) + "%";
		}).attr("transform", (datum) => {
			return "translate(" + labelArcGenerator.centroid(datum) + ")";
		}).style("text-anchor", "middle").attr("class", "slice");
		const titleText = group.append("text").text(db2.getDiagramTitle()).attr("x", 0).attr("y", -(height - 50) / 2).attr("class", "pieTitleText");
		const allSectionData = [...sections2.entries()].map(([label, value]) => ({
			label,
			value
		}));
		const legend = group.selectAll(".legend").data(allSectionData).enter().append("g").attr("class", "legend");
		legend.append("rect").attr("width", LEGEND_RECT_SIZE).attr("height", LEGEND_RECT_SIZE).style("fill", (d) => color(d.label)).style("stroke", (d) => color(d.label));
		legend.append("text").attr("x", LEGEND_RECT_SIZE + LEGEND_SPACING).attr("y", LEGEND_RECT_SIZE - LEGEND_SPACING).text((d) => {
			if (db2.getShowData()) return `${d.label} [${d.value}]`;
			return d.label;
		});
		const longestTextWidth = Math.max(...legend.selectAll("text").nodes().map((node) => node?.getBoundingClientRect().width ?? 0));
		let chartAndLegendHeight = height;
		let chartAndLegendWidth = pieWidth + MARGIN;
		const legendHeight = LEGEND_RECT_SIZE + LEGEND_SPACING;
		const totalLegendHeight = allSectionData.length * legendHeight;
		switch (legendPosition) {
			case "center":
				legend.attr("transform", (_datum, index) => {
					const offset = legendHeight * allSectionData.length / 2;
					const horizontal = -longestTextWidth / 2 - (LEGEND_RECT_SIZE + LEGEND_SPACING);
					const vertical = index * legendHeight - offset;
					return "translate(" + horizontal + "," + vertical + ")";
				});
				break;
			case "top":
				chartAndLegendHeight += totalLegendHeight;
				legend.attr("transform", (_datum, index) => {
					const offset = radius;
					return `translate(${-longestTextWidth / 2 - (LEGEND_RECT_SIZE + LEGEND_SPACING)}, ${index * legendHeight - offset})`;
				});
				pie.attr("transform", () => {
					return `translate(0, ${totalLegendHeight + legendHeight})`;
				});
				break;
			case "bottom":
				chartAndLegendHeight += totalLegendHeight;
				legend.attr("transform", (_datum, index) => {
					const offset = -radius - legendHeight;
					const horizontal = -longestTextWidth / 2 - (LEGEND_RECT_SIZE + LEGEND_SPACING);
					const vertical = index * legendHeight - offset;
					return "translate(" + horizontal + "," + vertical + ")";
				});
				break;
			case "left":
				chartAndLegendWidth += LEGEND_RECT_SIZE + LEGEND_SPACING + longestTextWidth;
				legend.attr("transform", (_datum, index) => {
					const offset = legendHeight * allSectionData.length / 2;
					const horizontal = -radius - (LEGEND_RECT_SIZE + LEGEND_SPACING);
					const vertical = index * legendHeight - offset;
					return "translate(" + horizontal + "," + vertical + ")";
				});
				pie.attr("transform", () => {
					return `translate(${longestTextWidth + LEGEND_RECT_SIZE + LEGEND_SPACING}, 0)`;
				});
				break;
			case "right":
			default:
				chartAndLegendWidth += LEGEND_RECT_SIZE + LEGEND_SPACING + longestTextWidth;
				legend.attr("transform", (_datum, index) => {
					const offset = legendHeight * allSectionData.length / 2;
					const horizontal = 12 * LEGEND_RECT_SIZE;
					const vertical = index * legendHeight - offset;
					return "translate(" + horizontal + "," + vertical + ")";
				});
				break;
		}
		const titleWidth = titleText.node()?.getBoundingClientRect().width ?? 0;
		const titleLeft = pieWidth / 2 - titleWidth / 2;
		const titleRight = pieWidth / 2 + titleWidth / 2;
		const viewBoxX = Math.min(0, titleLeft);
		const totalWidth = Math.max(chartAndLegendWidth, titleRight) - viewBoxX;
		svg.attr("viewBox", `${viewBoxX} 0 ${totalWidth} ${chartAndLegendHeight}`);
		configureSvgSize(svg, chartAndLegendHeight, totalWidth, pieConfig.useMaxWidth);
	}, "draw") },
	styles: pieStyles_default
};
export { diagram };
