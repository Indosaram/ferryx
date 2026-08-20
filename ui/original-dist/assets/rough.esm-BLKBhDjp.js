function t(t$1, e$1, s$1) {
	if (t$1 && t$1.length) {
		const [n$1, o$1] = e$1, a$1 = Math.PI / 180 * s$1, h$1 = Math.cos(a$1), r$1 = Math.sin(a$1);
		for (const e$2 of t$1) {
			const [t$2, s$2] = e$2;
			e$2[0] = (t$2 - n$1) * h$1 - (s$2 - o$1) * r$1 + n$1, e$2[1] = (t$2 - n$1) * r$1 + (s$2 - o$1) * h$1 + o$1;
		}
	}
}
function e(t$1, e$1) {
	return t$1[0] === e$1[0] && t$1[1] === e$1[1];
}
function s(s$1, n$1, o$1, a$1 = 1) {
	const h$1 = o$1, r$1 = Math.max(n$1, .1), i$1 = s$1[0] && s$1[0][0] && "number" == typeof s$1[0][0] ? [s$1] : s$1, c$1 = [0, 0];
	if (h$1) for (const e$1 of i$1) t(e$1, c$1, h$1);
	const l$1 = function(t$1, s$2, n$2) {
		const o$2 = [];
		for (const s$3 of t$1) {
			const t$2 = [...s$3];
			e(t$2[0], t$2[t$2.length - 1]) || t$2.push([t$2[0][0], t$2[0][1]]), t$2.length > 2 && o$2.push(t$2);
		}
		const a$2 = [];
		s$2 = Math.max(s$2, .1);
		const h$2 = [];
		for (const t$2 of o$2) for (let e$1 = 0; e$1 < t$2.length - 1; e$1++) {
			const s$3 = t$2[e$1], n$3 = t$2[e$1 + 1];
			if (s$3[1] !== n$3[1]) {
				const t$3 = Math.min(s$3[1], n$3[1]);
				h$2.push({
					ymin: t$3,
					ymax: Math.max(s$3[1], n$3[1]),
					x: t$3 === s$3[1] ? s$3[0] : n$3[0],
					islope: (n$3[0] - s$3[0]) / (n$3[1] - s$3[1])
				});
			}
		}
		if (h$2.sort(((t$2, e$1) => t$2.ymin < e$1.ymin ? -1 : t$2.ymin > e$1.ymin ? 1 : t$2.x < e$1.x ? -1 : t$2.x > e$1.x ? 1 : t$2.ymax === e$1.ymax ? 0 : (t$2.ymax - e$1.ymax) / Math.abs(t$2.ymax - e$1.ymax))), !h$2.length) return a$2;
		let r$2 = [], i$2 = h$2[0].ymin, c$2 = 0;
		for (; r$2.length || h$2.length;) {
			if (h$2.length) {
				let t$2 = -1;
				for (let e$1 = 0; e$1 < h$2.length && !(h$2[e$1].ymin > i$2); e$1++) t$2 = e$1;
				h$2.splice(0, t$2 + 1).forEach(((t$3) => {
					r$2.push({
						s: i$2,
						edge: t$3
					});
				}));
			}
			if (r$2 = r$2.filter(((t$2) => !(t$2.edge.ymax <= i$2))), r$2.sort(((t$2, e$1) => t$2.edge.x === e$1.edge.x ? 0 : (t$2.edge.x - e$1.edge.x) / Math.abs(t$2.edge.x - e$1.edge.x))), (1 !== n$2 || c$2 % s$2 == 0) && r$2.length > 1) for (let t$2 = 0; t$2 < r$2.length; t$2 += 2) {
				const e$1 = t$2 + 1;
				if (e$1 >= r$2.length) break;
				const s$3 = r$2[t$2].edge, n$3 = r$2[e$1].edge;
				a$2.push([[Math.round(s$3.x), i$2], [Math.round(n$3.x), i$2]]);
			}
			i$2 += n$2, r$2.forEach(((t$2) => {
				t$2.edge.x = t$2.edge.x + n$2 * t$2.edge.islope;
			})), c$2++;
		}
		return a$2;
	}(i$1, r$1, a$1);
	if (h$1) {
		for (const e$1 of i$1) t(e$1, c$1, -h$1);
		(function(e$1, s$2, n$2) {
			const o$2 = [];
			e$1.forEach(((t$1) => o$2.push(...t$1))), t(o$2, s$2, n$2);
		})(l$1, c$1, -h$1);
	}
	return l$1;
}
function n(t$1, e$1) {
	var n$1;
	const o$1 = e$1.hachureAngle + 90;
	let a$1 = e$1.hachureGap;
	a$1 < 0 && (a$1 = 4 * e$1.strokeWidth), a$1 = Math.round(Math.max(a$1, .1));
	let h$1 = 1;
	return e$1.roughness >= 1 && ((null === (n$1 = e$1.randomizer) || void 0 === n$1 ? void 0 : n$1.next()) || Math.random()) > .7 && (h$1 = a$1), s(t$1, a$1, o$1, h$1 || 1);
}
var o = class {
	constructor(t$1) {
		this.helper = t$1;
	}
	fillPolygons(t$1, e$1) {
		return this._fillPolygons(t$1, e$1);
	}
	_fillPolygons(t$1, e$1) {
		const s$1 = n(t$1, e$1);
		return {
			type: "fillSketch",
			ops: this.renderLines(s$1, e$1)
		};
	}
	renderLines(t$1, e$1) {
		const s$1 = [];
		for (const n$1 of t$1) s$1.push(...this.helper.doubleLineOps(n$1[0][0], n$1[0][1], n$1[1][0], n$1[1][1], e$1));
		return s$1;
	}
};
function a(t$1) {
	const e$1 = t$1[0], s$1 = t$1[1];
	return Math.sqrt(Math.pow(e$1[0] - s$1[0], 2) + Math.pow(e$1[1] - s$1[1], 2));
}
var h = class extends o {
	fillPolygons(t$1, e$1) {
		let s$1 = e$1.hachureGap;
		s$1 < 0 && (s$1 = 4 * e$1.strokeWidth), s$1 = Math.max(s$1, .1);
		const o$1 = n(t$1, Object.assign({}, e$1, { hachureGap: s$1 })), h$1 = Math.PI / 180 * e$1.hachureAngle, r$1 = [], i$1 = .5 * s$1 * Math.cos(h$1), c$1 = .5 * s$1 * Math.sin(h$1);
		for (const [t$2, e$2] of o$1) a([t$2, e$2]) && r$1.push([[t$2[0] - i$1, t$2[1] + c$1], [...e$2]], [[t$2[0] + i$1, t$2[1] - c$1], [...e$2]]);
		return {
			type: "fillSketch",
			ops: this.renderLines(r$1, e$1)
		};
	}
};
var r = class extends o {
	fillPolygons(t$1, e$1) {
		const s$1 = this._fillPolygons(t$1, e$1), n$1 = Object.assign({}, e$1, { hachureAngle: e$1.hachureAngle + 90 }), o$1 = this._fillPolygons(t$1, n$1);
		return s$1.ops = s$1.ops.concat(o$1.ops), s$1;
	}
};
var i = class {
	constructor(t$1) {
		this.helper = t$1;
	}
	fillPolygons(t$1, e$1) {
		const s$1 = n(t$1, e$1 = Object.assign({}, e$1, { hachureAngle: 0 }));
		return this.dotsOnLines(s$1, e$1);
	}
	dotsOnLines(t$1, e$1) {
		const s$1 = [];
		let n$1 = e$1.hachureGap;
		n$1 < 0 && (n$1 = 4 * e$1.strokeWidth), n$1 = Math.max(n$1, .1);
		let o$1 = e$1.fillWeight;
		o$1 < 0 && (o$1 = e$1.strokeWidth / 2);
		const h$1 = n$1 / 4;
		for (const r$1 of t$1) {
			const t$2 = a(r$1), i$1 = t$2 / n$1, c$1 = Math.ceil(i$1) - 1, l$1 = t$2 - c$1 * n$1, u$1 = (r$1[0][0] + r$1[1][0]) / 2 - n$1 / 4, p$1 = Math.min(r$1[0][1], r$1[1][1]);
			for (let t$3 = 0; t$3 < c$1; t$3++) {
				const a$1 = p$1 + l$1 + t$3 * n$1, r$2 = u$1 - h$1 + 2 * Math.random() * h$1, i$2 = a$1 - h$1 + 2 * Math.random() * h$1, c$2 = this.helper.ellipse(r$2, i$2, o$1, o$1, e$1);
				s$1.push(...c$2.ops);
			}
		}
		return {
			type: "fillSketch",
			ops: s$1
		};
	}
};
var c = class {
	constructor(t$1) {
		this.helper = t$1;
	}
	fillPolygons(t$1, e$1) {
		const s$1 = n(t$1, e$1);
		return {
			type: "fillSketch",
			ops: this.dashedLine(s$1, e$1)
		};
	}
	dashedLine(t$1, e$1) {
		const s$1 = e$1.dashOffset < 0 ? e$1.hachureGap < 0 ? 4 * e$1.strokeWidth : e$1.hachureGap : e$1.dashOffset, n$1 = e$1.dashGap < 0 ? e$1.hachureGap < 0 ? 4 * e$1.strokeWidth : e$1.hachureGap : e$1.dashGap, o$1 = [];
		return t$1.forEach(((t$2) => {
			const h$1 = a(t$2), r$1 = Math.floor(h$1 / (s$1 + n$1)), i$1 = (h$1 + n$1 - r$1 * (s$1 + n$1)) / 2;
			let c$1 = t$2[0], l$1 = t$2[1];
			c$1[0] > l$1[0] && (c$1 = t$2[1], l$1 = t$2[0]);
			const u$1 = Math.atan((l$1[1] - c$1[1]) / (l$1[0] - c$1[0]));
			for (let t$3 = 0; t$3 < r$1; t$3++) {
				const a$1 = t$3 * (s$1 + n$1), h$2 = a$1 + s$1, r$2 = [c$1[0] + a$1 * Math.cos(u$1) + i$1 * Math.cos(u$1), c$1[1] + a$1 * Math.sin(u$1) + i$1 * Math.sin(u$1)], l$2 = [c$1[0] + h$2 * Math.cos(u$1) + i$1 * Math.cos(u$1), c$1[1] + h$2 * Math.sin(u$1) + i$1 * Math.sin(u$1)];
				o$1.push(...this.helper.doubleLineOps(r$2[0], r$2[1], l$2[0], l$2[1], e$1));
			}
		})), o$1;
	}
};
var l = class {
	constructor(t$1) {
		this.helper = t$1;
	}
	fillPolygons(t$1, e$1) {
		const s$1 = e$1.hachureGap < 0 ? 4 * e$1.strokeWidth : e$1.hachureGap, o$1 = e$1.zigzagOffset < 0 ? s$1 : e$1.zigzagOffset, a$1 = n(t$1, e$1 = Object.assign({}, e$1, { hachureGap: s$1 + o$1 }));
		return {
			type: "fillSketch",
			ops: this.zigzagLines(a$1, o$1, e$1)
		};
	}
	zigzagLines(t$1, e$1, s$1) {
		const n$1 = [];
		return t$1.forEach(((t$2) => {
			const o$1 = a(t$2), h$1 = Math.round(o$1 / (2 * e$1));
			let r$1 = t$2[0], i$1 = t$2[1];
			r$1[0] > i$1[0] && (r$1 = t$2[1], i$1 = t$2[0]);
			const c$1 = Math.atan((i$1[1] - r$1[1]) / (i$1[0] - r$1[0]));
			for (let t$3 = 0; t$3 < h$1; t$3++) {
				const o$2 = 2 * t$3 * e$1, a$1 = 2 * (t$3 + 1) * e$1, h$2 = Math.sqrt(2 * Math.pow(e$1, 2)), i$2 = [r$1[0] + o$2 * Math.cos(c$1), r$1[1] + o$2 * Math.sin(c$1)], l$1 = [r$1[0] + a$1 * Math.cos(c$1), r$1[1] + a$1 * Math.sin(c$1)], u$1 = [i$2[0] + h$2 * Math.cos(c$1 + Math.PI / 4), i$2[1] + h$2 * Math.sin(c$1 + Math.PI / 4)];
				n$1.push(...this.helper.doubleLineOps(i$2[0], i$2[1], u$1[0], u$1[1], s$1), ...this.helper.doubleLineOps(u$1[0], u$1[1], l$1[0], l$1[1], s$1));
			}
		})), n$1;
	}
};
var u = {};
var p = class {
	constructor(t$1) {
		this.seed = t$1;
	}
	next() {
		return this.seed ? (2 ** 31 - 1 & (this.seed = Math.imul(48271, this.seed))) / 2 ** 31 : Math.random();
	}
};
var f = 0, d = 1, g = 2, M = {
	A: 7,
	a: 7,
	C: 6,
	c: 6,
	H: 1,
	h: 1,
	L: 2,
	l: 2,
	M: 2,
	m: 2,
	Q: 4,
	q: 4,
	S: 4,
	s: 4,
	T: 2,
	t: 2,
	V: 1,
	v: 1,
	Z: 0,
	z: 0
};
function k(t$1, e$1) {
	return t$1.type === e$1;
}
function b(t$1) {
	const e$1 = [], s$1 = function(t$2) {
		const e$2 = new Array();
		for (; "" !== t$2;) if (t$2.match(/^([ \t\r\n,]+)/)) t$2 = t$2.substr(RegExp.$1.length);
		else if (t$2.match(/^([aAcChHlLmMqQsStTvVzZ])/)) e$2[e$2.length] = {
			type: f,
			text: RegExp.$1
		}, t$2 = t$2.substr(RegExp.$1.length);
		else {
			if (!t$2.match(/^(([-+]?[0-9]+(\.[0-9]*)?|[-+]?\.[0-9]+)([eE][-+]?[0-9]+)?)/)) return [];
			e$2[e$2.length] = {
				type: d,
				text: `${parseFloat(RegExp.$1)}`
			}, t$2 = t$2.substr(RegExp.$1.length);
		}
		return e$2[e$2.length] = {
			type: g,
			text: ""
		}, e$2;
	}(t$1);
	let n$1 = "BOD", o$1 = 0, a$1 = s$1[o$1];
	for (; !k(a$1, g);) {
		let h$1 = 0;
		const r$1 = [];
		if ("BOD" === n$1) {
			if ("M" !== a$1.text && "m" !== a$1.text) return b("M0,0" + t$1);
			o$1++, h$1 = M[a$1.text], n$1 = a$1.text;
		} else k(a$1, d) ? h$1 = M[n$1] : (o$1++, h$1 = M[a$1.text], n$1 = a$1.text);
		if (!(o$1 + h$1 < s$1.length)) throw new Error("Path data ended short");
		for (let t$2 = o$1; t$2 < o$1 + h$1; t$2++) {
			const e$2 = s$1[t$2];
			if (!k(e$2, d)) throw new Error("Param not a number: " + n$1 + "," + e$2.text);
			r$1[r$1.length] = +e$2.text;
		}
		if ("number" != typeof M[n$1]) throw new Error("Bad segment: " + n$1);
		{
			const t$2 = {
				key: n$1,
				data: r$1
			};
			e$1.push(t$2), o$1 += h$1, a$1 = s$1[o$1], "M" === n$1 && (n$1 = "L"), "m" === n$1 && (n$1 = "l");
		}
	}
	return e$1;
}
function y(t$1) {
	let e$1 = 0, s$1 = 0, n$1 = 0, o$1 = 0;
	const a$1 = [];
	for (const { key: h$1, data: r$1 } of t$1) switch (h$1) {
		case "M":
			a$1.push({
				key: "M",
				data: [...r$1]
			}), [e$1, s$1] = r$1, [n$1, o$1] = r$1;
			break;
		case "m":
			e$1 += r$1[0], s$1 += r$1[1], a$1.push({
				key: "M",
				data: [e$1, s$1]
			}), n$1 = e$1, o$1 = s$1;
			break;
		case "L":
			a$1.push({
				key: "L",
				data: [...r$1]
			}), [e$1, s$1] = r$1;
			break;
		case "l":
			e$1 += r$1[0], s$1 += r$1[1], a$1.push({
				key: "L",
				data: [e$1, s$1]
			});
			break;
		case "C":
			a$1.push({
				key: "C",
				data: [...r$1]
			}), e$1 = r$1[4], s$1 = r$1[5];
			break;
		case "c": {
			const t$2 = r$1.map(((t$3, n$2) => n$2 % 2 ? t$3 + s$1 : t$3 + e$1));
			a$1.push({
				key: "C",
				data: t$2
			}), e$1 = t$2[4], s$1 = t$2[5];
			break;
		}
		case "Q":
			a$1.push({
				key: "Q",
				data: [...r$1]
			}), e$1 = r$1[2], s$1 = r$1[3];
			break;
		case "q": {
			const t$2 = r$1.map(((t$3, n$2) => n$2 % 2 ? t$3 + s$1 : t$3 + e$1));
			a$1.push({
				key: "Q",
				data: t$2
			}), e$1 = t$2[2], s$1 = t$2[3];
			break;
		}
		case "A":
			a$1.push({
				key: "A",
				data: [...r$1]
			}), e$1 = r$1[5], s$1 = r$1[6];
			break;
		case "a":
			e$1 += r$1[5], s$1 += r$1[6], a$1.push({
				key: "A",
				data: [
					r$1[0],
					r$1[1],
					r$1[2],
					r$1[3],
					r$1[4],
					e$1,
					s$1
				]
			});
			break;
		case "H":
			a$1.push({
				key: "H",
				data: [...r$1]
			}), e$1 = r$1[0];
			break;
		case "h":
			e$1 += r$1[0], a$1.push({
				key: "H",
				data: [e$1]
			});
			break;
		case "V":
			a$1.push({
				key: "V",
				data: [...r$1]
			}), s$1 = r$1[0];
			break;
		case "v":
			s$1 += r$1[0], a$1.push({
				key: "V",
				data: [s$1]
			});
			break;
		case "S":
			a$1.push({
				key: "S",
				data: [...r$1]
			}), e$1 = r$1[2], s$1 = r$1[3];
			break;
		case "s": {
			const t$2 = r$1.map(((t$3, n$2) => n$2 % 2 ? t$3 + s$1 : t$3 + e$1));
			a$1.push({
				key: "S",
				data: t$2
			}), e$1 = t$2[2], s$1 = t$2[3];
			break;
		}
		case "T":
			a$1.push({
				key: "T",
				data: [...r$1]
			}), e$1 = r$1[0], s$1 = r$1[1];
			break;
		case "t":
			e$1 += r$1[0], s$1 += r$1[1], a$1.push({
				key: "T",
				data: [e$1, s$1]
			});
			break;
		case "Z":
		case "z": a$1.push({
			key: "Z",
			data: []
		}), e$1 = n$1, s$1 = o$1;
	}
	return a$1;
}
function m(t$1) {
	const e$1 = [];
	let s$1 = "", n$1 = 0, o$1 = 0, a$1 = 0, h$1 = 0, r$1 = 0, i$1 = 0;
	for (const { key: c$1, data: l$1 } of t$1) {
		switch (c$1) {
			case "M":
				e$1.push({
					key: "M",
					data: [...l$1]
				}), [n$1, o$1] = l$1, [a$1, h$1] = l$1;
				break;
			case "C":
				e$1.push({
					key: "C",
					data: [...l$1]
				}), n$1 = l$1[4], o$1 = l$1[5], r$1 = l$1[2], i$1 = l$1[3];
				break;
			case "L":
				e$1.push({
					key: "L",
					data: [...l$1]
				}), [n$1, o$1] = l$1;
				break;
			case "H":
				n$1 = l$1[0], e$1.push({
					key: "L",
					data: [n$1, o$1]
				});
				break;
			case "V":
				o$1 = l$1[0], e$1.push({
					key: "L",
					data: [n$1, o$1]
				});
				break;
			case "S": {
				let t$2 = 0, a$2 = 0;
				"C" === s$1 || "S" === s$1 ? (t$2 = n$1 + (n$1 - r$1), a$2 = o$1 + (o$1 - i$1)) : (t$2 = n$1, a$2 = o$1), e$1.push({
					key: "C",
					data: [
						t$2,
						a$2,
						...l$1
					]
				}), r$1 = l$1[0], i$1 = l$1[1], n$1 = l$1[2], o$1 = l$1[3];
				break;
			}
			case "T": {
				const [t$2, a$2] = l$1;
				let h$2 = 0, c$2 = 0;
				"Q" === s$1 || "T" === s$1 ? (h$2 = n$1 + (n$1 - r$1), c$2 = o$1 + (o$1 - i$1)) : (h$2 = n$1, c$2 = o$1);
				const u$1 = n$1 + 2 * (h$2 - n$1) / 3, p$1 = o$1 + 2 * (c$2 - o$1) / 3, f$1 = t$2 + 2 * (h$2 - t$2) / 3, d$1 = a$2 + 2 * (c$2 - a$2) / 3;
				e$1.push({
					key: "C",
					data: [
						u$1,
						p$1,
						f$1,
						d$1,
						t$2,
						a$2
					]
				}), r$1 = h$2, i$1 = c$2, n$1 = t$2, o$1 = a$2;
				break;
			}
			case "Q": {
				const [t$2, s$2, a$2, h$2] = l$1, c$2 = n$1 + 2 * (t$2 - n$1) / 3, u$1 = o$1 + 2 * (s$2 - o$1) / 3, p$1 = a$2 + 2 * (t$2 - a$2) / 3, f$1 = h$2 + 2 * (s$2 - h$2) / 3;
				e$1.push({
					key: "C",
					data: [
						c$2,
						u$1,
						p$1,
						f$1,
						a$2,
						h$2
					]
				}), r$1 = t$2, i$1 = s$2, n$1 = a$2, o$1 = h$2;
				break;
			}
			case "A": {
				const t$2 = Math.abs(l$1[0]), s$2 = Math.abs(l$1[1]), a$2 = l$1[2], h$2 = l$1[3], r$2 = l$1[4], i$2 = l$1[5], c$2 = l$1[6];
				if (0 === t$2 || 0 === s$2) e$1.push({
					key: "C",
					data: [
						n$1,
						o$1,
						i$2,
						c$2,
						i$2,
						c$2
					]
				}), n$1 = i$2, o$1 = c$2;
				else if (n$1 !== i$2 || o$1 !== c$2) x(n$1, o$1, i$2, c$2, t$2, s$2, a$2, h$2, r$2).forEach((function(t$3) {
					e$1.push({
						key: "C",
						data: t$3
					});
				})), n$1 = i$2, o$1 = c$2;
				break;
			}
			case "Z": e$1.push({
				key: "Z",
				data: []
			}), n$1 = a$1, o$1 = h$1;
		}
		s$1 = c$1;
	}
	return e$1;
}
function w(t$1, e$1, s$1) {
	return [t$1 * Math.cos(s$1) - e$1 * Math.sin(s$1), t$1 * Math.sin(s$1) + e$1 * Math.cos(s$1)];
}
function x(t$1, e$1, s$1, n$1, o$1, a$1, h$1, r$1, i$1, c$1) {
	const l$1 = (u$1 = h$1, Math.PI * u$1 / 180);
	var u$1;
	let p$1 = [], f$1 = 0, d$1 = 0, g$1 = 0, M$1 = 0;
	if (c$1) [f$1, d$1, g$1, M$1] = c$1;
	else {
		[t$1, e$1] = w(t$1, e$1, -l$1), [s$1, n$1] = w(s$1, n$1, -l$1);
		const h$2 = (t$1 - s$1) / 2, c$2 = (e$1 - n$1) / 2;
		let u$2 = h$2 * h$2 / (o$1 * o$1) + c$2 * c$2 / (a$1 * a$1);
		u$2 > 1 && (u$2 = Math.sqrt(u$2), o$1 *= u$2, a$1 *= u$2);
		const p$2 = o$1 * o$1, k$2 = a$1 * a$1, b$2 = p$2 * k$2 - p$2 * c$2 * c$2 - k$2 * h$2 * h$2, y$2 = p$2 * c$2 * c$2 + k$2 * h$2 * h$2, m$2 = (r$1 === i$1 ? -1 : 1) * Math.sqrt(Math.abs(b$2 / y$2));
		g$1 = m$2 * o$1 * c$2 / a$1 + (t$1 + s$1) / 2, M$1 = m$2 * -a$1 * h$2 / o$1 + (e$1 + n$1) / 2, f$1 = Math.asin(parseFloat(((e$1 - M$1) / a$1).toFixed(9))), d$1 = Math.asin(parseFloat(((n$1 - M$1) / a$1).toFixed(9))), t$1 < g$1 && (f$1 = Math.PI - f$1), s$1 < g$1 && (d$1 = Math.PI - d$1), f$1 < 0 && (f$1 = 2 * Math.PI + f$1), d$1 < 0 && (d$1 = 2 * Math.PI + d$1), i$1 && f$1 > d$1 && (f$1 -= 2 * Math.PI), !i$1 && d$1 > f$1 && (d$1 -= 2 * Math.PI);
	}
	let k$1 = d$1 - f$1;
	if (Math.abs(k$1) > 120 * Math.PI / 180) {
		const t$2 = d$1, e$2 = s$1, r$2 = n$1;
		d$1 = i$1 && d$1 > f$1 ? f$1 + 120 * Math.PI / 180 * 1 : f$1 + 120 * Math.PI / 180 * -1, p$1 = x(s$1 = g$1 + o$1 * Math.cos(d$1), n$1 = M$1 + a$1 * Math.sin(d$1), e$2, r$2, o$1, a$1, h$1, 0, i$1, [
			d$1,
			t$2,
			g$1,
			M$1
		]);
	}
	k$1 = d$1 - f$1;
	const b$1 = Math.cos(f$1), y$1 = Math.sin(f$1), m$1 = Math.cos(d$1), P$1 = Math.sin(d$1), v$1 = Math.tan(k$1 / 4), S$1 = 4 / 3 * o$1 * v$1, O$1 = 4 / 3 * a$1 * v$1, L$1 = [t$1, e$1], T$1 = [t$1 + S$1 * y$1, e$1 - O$1 * b$1], D$1 = [s$1 + S$1 * P$1, n$1 - O$1 * m$1], A$1 = [s$1, n$1];
	if (T$1[0] = 2 * L$1[0] - T$1[0], T$1[1] = 2 * L$1[1] - T$1[1], c$1) return [
		T$1,
		D$1,
		A$1
	].concat(p$1);
	{
		p$1 = [
			T$1,
			D$1,
			A$1
		].concat(p$1);
		const t$2 = [];
		for (let e$2 = 0; e$2 < p$1.length; e$2 += 3) {
			const s$2 = w(p$1[e$2][0], p$1[e$2][1], l$1), n$2 = w(p$1[e$2 + 1][0], p$1[e$2 + 1][1], l$1), o$2 = w(p$1[e$2 + 2][0], p$1[e$2 + 2][1], l$1);
			t$2.push([
				s$2[0],
				s$2[1],
				n$2[0],
				n$2[1],
				o$2[0],
				o$2[1]
			]);
		}
		return t$2;
	}
}
var P = {
	randOffset: function(t$1, e$1) {
		return G(t$1, e$1);
	},
	randOffsetWithRange: function(t$1, e$1, s$1) {
		return E(t$1, e$1, s$1);
	},
	ellipse: function(t$1, e$1, s$1, n$1, o$1) {
		return D(t$1, e$1, o$1, T(s$1, n$1, o$1)).opset;
	},
	doubleLineOps: function(t$1, e$1, s$1, n$1, o$1) {
		return $(t$1, e$1, s$1, n$1, o$1, !0);
	}
};
function v(t$1, e$1, s$1, n$1, o$1) {
	return {
		type: "path",
		ops: $(t$1, e$1, s$1, n$1, o$1)
	};
}
function S(t$1, e$1, s$1) {
	const n$1 = (t$1 || []).length;
	if (n$1 > 2) {
		const o$1 = [];
		for (let e$2 = 0; e$2 < n$1 - 1; e$2++) o$1.push(...$(t$1[e$2][0], t$1[e$2][1], t$1[e$2 + 1][0], t$1[e$2 + 1][1], s$1));
		return e$1 && o$1.push(...$(t$1[n$1 - 1][0], t$1[n$1 - 1][1], t$1[0][0], t$1[0][1], s$1)), {
			type: "path",
			ops: o$1
		};
	}
	return 2 === n$1 ? v(t$1[0][0], t$1[0][1], t$1[1][0], t$1[1][1], s$1) : {
		type: "path",
		ops: []
	};
}
function O(t$1, e$1, s$1, n$1, o$1) {
	return function(t$2, e$2) {
		return S(t$2, !0, e$2);
	}([
		[t$1, e$1],
		[t$1 + s$1, e$1],
		[t$1 + s$1, e$1 + n$1],
		[t$1, e$1 + n$1]
	], o$1);
}
function L(t$1, e$1) {
	if (t$1.length) {
		const s$1 = "number" == typeof t$1[0][0] ? [t$1] : t$1, n$1 = j(s$1[0], 1 * (1 + .2 * e$1.roughness), e$1), o$1 = e$1.disableMultiStroke ? [] : j(s$1[0], 1.5 * (1 + .22 * e$1.roughness), z(e$1));
		for (let t$2 = 1; t$2 < s$1.length; t$2++) {
			const a$1 = s$1[t$2];
			if (a$1.length) {
				const t$3 = j(a$1, 1 * (1 + .2 * e$1.roughness), e$1), s$2 = e$1.disableMultiStroke ? [] : j(a$1, 1.5 * (1 + .22 * e$1.roughness), z(e$1));
				for (const e$2 of t$3) "move" !== e$2.op && n$1.push(e$2);
				for (const t$4 of s$2) "move" !== t$4.op && o$1.push(t$4);
			}
		}
		return {
			type: "path",
			ops: n$1.concat(o$1)
		};
	}
	return {
		type: "path",
		ops: []
	};
}
function T(t$1, e$1, s$1) {
	const n$1 = Math.sqrt(2 * Math.PI * Math.sqrt((Math.pow(t$1 / 2, 2) + Math.pow(e$1 / 2, 2)) / 2)), o$1 = Math.ceil(Math.max(s$1.curveStepCount, s$1.curveStepCount / Math.sqrt(200) * n$1)), a$1 = 2 * Math.PI / o$1;
	let h$1 = Math.abs(t$1 / 2), r$1 = Math.abs(e$1 / 2);
	const i$1 = 1 - s$1.curveFitting;
	return h$1 += G(h$1 * i$1, s$1), r$1 += G(r$1 * i$1, s$1), {
		increment: a$1,
		rx: h$1,
		ry: r$1
	};
}
function D(t$1, e$1, s$1, n$1) {
	const [o$1, a$1] = F(n$1.increment, t$1, e$1, n$1.rx, n$1.ry, 1, n$1.increment * E(.1, E(.4, 1, s$1), s$1), s$1);
	let h$1 = q(o$1, null, s$1);
	if (!s$1.disableMultiStroke && 0 !== s$1.roughness) {
		const [o$2] = F(n$1.increment, t$1, e$1, n$1.rx, n$1.ry, 1.5, 0, s$1), a$2 = q(o$2, null, s$1);
		h$1 = h$1.concat(a$2);
	}
	return {
		estimatedPoints: a$1,
		opset: {
			type: "path",
			ops: h$1
		}
	};
}
function A(t$1, e$1, s$1, n$1, o$1, a$1, h$1, r$1, i$1) {
	const c$1 = t$1, l$1 = e$1;
	let u$1 = Math.abs(s$1 / 2), p$1 = Math.abs(n$1 / 2);
	u$1 += G(.01 * u$1, i$1), p$1 += G(.01 * p$1, i$1);
	let f$1 = o$1, d$1 = a$1;
	for (; f$1 < 0;) f$1 += 2 * Math.PI, d$1 += 2 * Math.PI;
	d$1 - f$1 > 2 * Math.PI && (f$1 = 0, d$1 = 2 * Math.PI);
	const g$1 = 2 * Math.PI / i$1.curveStepCount, M$1 = Math.min(g$1 / 2, (d$1 - f$1) / 2), k$1 = V(M$1, c$1, l$1, u$1, p$1, f$1, d$1, 1, i$1);
	if (!i$1.disableMultiStroke) {
		const t$2 = V(M$1, c$1, l$1, u$1, p$1, f$1, d$1, 1.5, i$1);
		k$1.push(...t$2);
	}
	return h$1 && (r$1 ? k$1.push(...$(c$1, l$1, c$1 + u$1 * Math.cos(f$1), l$1 + p$1 * Math.sin(f$1), i$1), ...$(c$1, l$1, c$1 + u$1 * Math.cos(d$1), l$1 + p$1 * Math.sin(d$1), i$1)) : k$1.push({
		op: "lineTo",
		data: [c$1, l$1]
	}, {
		op: "lineTo",
		data: [c$1 + u$1 * Math.cos(f$1), l$1 + p$1 * Math.sin(f$1)]
	})), {
		type: "path",
		ops: k$1
	};
}
function _(t$1, e$1) {
	const s$1 = m(y(b(t$1))), n$1 = [];
	let o$1 = [0, 0], a$1 = [0, 0];
	for (const { key: t$2, data: h$1 } of s$1) switch (t$2) {
		case "M":
			a$1 = [h$1[0], h$1[1]], o$1 = [h$1[0], h$1[1]];
			break;
		case "L":
			n$1.push(...$(a$1[0], a$1[1], h$1[0], h$1[1], e$1)), a$1 = [h$1[0], h$1[1]];
			break;
		case "C": {
			const [t$3, s$2, o$2, r$1, i$1, c$1] = h$1;
			n$1.push(...Z(t$3, s$2, o$2, r$1, i$1, c$1, a$1, e$1)), a$1 = [i$1, c$1];
			break;
		}
		case "Z": n$1.push(...$(a$1[0], a$1[1], o$1[0], o$1[1], e$1)), a$1 = [o$1[0], o$1[1]];
	}
	return {
		type: "path",
		ops: n$1
	};
}
function I(t$1, e$1) {
	const s$1 = [];
	for (const n$1 of t$1) if (n$1.length) {
		const t$2 = e$1.maxRandomnessOffset || 0, o$1 = n$1.length;
		if (o$1 > 2) {
			s$1.push({
				op: "move",
				data: [n$1[0][0] + G(t$2, e$1), n$1[0][1] + G(t$2, e$1)]
			});
			for (let a$1 = 1; a$1 < o$1; a$1++) s$1.push({
				op: "lineTo",
				data: [n$1[a$1][0] + G(t$2, e$1), n$1[a$1][1] + G(t$2, e$1)]
			});
		}
	}
	return {
		type: "fillPath",
		ops: s$1
	};
}
function C(t$1, e$1) {
	return function(t$2, e$2) {
		let s$1 = t$2.fillStyle || "hachure";
		if (!u[s$1]) switch (s$1) {
			case "zigzag":
				u[s$1] || (u[s$1] = new h(e$2));
				break;
			case "cross-hatch":
				u[s$1] || (u[s$1] = new r(e$2));
				break;
			case "dots":
				u[s$1] || (u[s$1] = new i(e$2));
				break;
			case "dashed":
				u[s$1] || (u[s$1] = new c(e$2));
				break;
			case "zigzag-line":
				u[s$1] || (u[s$1] = new l(e$2));
				break;
			default: s$1 = "hachure", u[s$1] || (u[s$1] = new o(e$2));
		}
		return u[s$1];
	}(e$1, P).fillPolygons(t$1, e$1);
}
function z(t$1) {
	const e$1 = Object.assign({}, t$1);
	return e$1.randomizer = void 0, t$1.seed && (e$1.seed = t$1.seed + 1), e$1;
}
function W(t$1) {
	return t$1.randomizer || (t$1.randomizer = new p(t$1.seed || 0)), t$1.randomizer.next();
}
function E(t$1, e$1, s$1, n$1 = 1) {
	return s$1.roughness * n$1 * (W(s$1) * (e$1 - t$1) + t$1);
}
function G(t$1, e$1, s$1 = 1) {
	return E(-t$1, t$1, e$1, s$1);
}
function $(t$1, e$1, s$1, n$1, o$1, a$1 = !1) {
	const h$1 = a$1 ? o$1.disableMultiStrokeFill : o$1.disableMultiStroke, r$1 = R(t$1, e$1, s$1, n$1, o$1, !0, !1);
	if (h$1) return r$1;
	const i$1 = R(t$1, e$1, s$1, n$1, o$1, !0, !0);
	return r$1.concat(i$1);
}
function R(t$1, e$1, s$1, n$1, o$1, a$1, h$1) {
	const r$1 = Math.pow(t$1 - s$1, 2) + Math.pow(e$1 - n$1, 2), i$1 = Math.sqrt(r$1);
	let c$1 = 1;
	c$1 = i$1 < 200 ? 1 : i$1 > 500 ? .4 : -.0016668 * i$1 + 1.233334;
	let l$1 = o$1.maxRandomnessOffset || 0;
	l$1 * l$1 * 100 > r$1 && (l$1 = i$1 / 10);
	const u$1 = l$1 / 2, p$1 = .2 + .2 * W(o$1);
	let f$1 = o$1.bowing * o$1.maxRandomnessOffset * (n$1 - e$1) / 200, d$1 = o$1.bowing * o$1.maxRandomnessOffset * (t$1 - s$1) / 200;
	f$1 = G(f$1, o$1, c$1), d$1 = G(d$1, o$1, c$1);
	const g$1 = [], M$1 = () => G(u$1, o$1, c$1), k$1 = () => G(l$1, o$1, c$1), b$1 = o$1.preserveVertices;
	return a$1 && (h$1 ? g$1.push({
		op: "move",
		data: [t$1 + (b$1 ? 0 : M$1()), e$1 + (b$1 ? 0 : M$1())]
	}) : g$1.push({
		op: "move",
		data: [t$1 + (b$1 ? 0 : G(l$1, o$1, c$1)), e$1 + (b$1 ? 0 : G(l$1, o$1, c$1))]
	})), h$1 ? g$1.push({
		op: "bcurveTo",
		data: [
			f$1 + t$1 + (s$1 - t$1) * p$1 + M$1(),
			d$1 + e$1 + (n$1 - e$1) * p$1 + M$1(),
			f$1 + t$1 + 2 * (s$1 - t$1) * p$1 + M$1(),
			d$1 + e$1 + 2 * (n$1 - e$1) * p$1 + M$1(),
			s$1 + (b$1 ? 0 : M$1()),
			n$1 + (b$1 ? 0 : M$1())
		]
	}) : g$1.push({
		op: "bcurveTo",
		data: [
			f$1 + t$1 + (s$1 - t$1) * p$1 + k$1(),
			d$1 + e$1 + (n$1 - e$1) * p$1 + k$1(),
			f$1 + t$1 + 2 * (s$1 - t$1) * p$1 + k$1(),
			d$1 + e$1 + 2 * (n$1 - e$1) * p$1 + k$1(),
			s$1 + (b$1 ? 0 : k$1()),
			n$1 + (b$1 ? 0 : k$1())
		]
	}), g$1;
}
function j(t$1, e$1, s$1) {
	if (!t$1.length) return [];
	const n$1 = [];
	n$1.push([t$1[0][0] + G(e$1, s$1), t$1[0][1] + G(e$1, s$1)]), n$1.push([t$1[0][0] + G(e$1, s$1), t$1[0][1] + G(e$1, s$1)]);
	for (let o$1 = 1; o$1 < t$1.length; o$1++) n$1.push([t$1[o$1][0] + G(e$1, s$1), t$1[o$1][1] + G(e$1, s$1)]), o$1 === t$1.length - 1 && n$1.push([t$1[o$1][0] + G(e$1, s$1), t$1[o$1][1] + G(e$1, s$1)]);
	return q(n$1, null, s$1);
}
function q(t$1, e$1, s$1) {
	const n$1 = t$1.length, o$1 = [];
	if (n$1 > 3) {
		const a$1 = [], h$1 = 1 - s$1.curveTightness;
		o$1.push({
			op: "move",
			data: [t$1[1][0], t$1[1][1]]
		});
		for (let e$2 = 1; e$2 + 2 < n$1; e$2++) {
			const s$2 = t$1[e$2];
			a$1[0] = [s$2[0], s$2[1]], a$1[1] = [s$2[0] + (h$1 * t$1[e$2 + 1][0] - h$1 * t$1[e$2 - 1][0]) / 6, s$2[1] + (h$1 * t$1[e$2 + 1][1] - h$1 * t$1[e$2 - 1][1]) / 6], a$1[2] = [t$1[e$2 + 1][0] + (h$1 * t$1[e$2][0] - h$1 * t$1[e$2 + 2][0]) / 6, t$1[e$2 + 1][1] + (h$1 * t$1[e$2][1] - h$1 * t$1[e$2 + 2][1]) / 6], a$1[3] = [t$1[e$2 + 1][0], t$1[e$2 + 1][1]], o$1.push({
				op: "bcurveTo",
				data: [
					a$1[1][0],
					a$1[1][1],
					a$1[2][0],
					a$1[2][1],
					a$1[3][0],
					a$1[3][1]
				]
			});
		}
		if (e$1 && 2 === e$1.length) {
			const t$2 = s$1.maxRandomnessOffset;
			o$1.push({
				op: "lineTo",
				data: [e$1[0] + G(t$2, s$1), e$1[1] + G(t$2, s$1)]
			});
		}
	} else 3 === n$1 ? (o$1.push({
		op: "move",
		data: [t$1[1][0], t$1[1][1]]
	}), o$1.push({
		op: "bcurveTo",
		data: [
			t$1[1][0],
			t$1[1][1],
			t$1[2][0],
			t$1[2][1],
			t$1[2][0],
			t$1[2][1]
		]
	})) : 2 === n$1 && o$1.push(...R(t$1[0][0], t$1[0][1], t$1[1][0], t$1[1][1], s$1, !0, !0));
	return o$1;
}
function F(t$1, e$1, s$1, n$1, o$1, a$1, h$1, r$1) {
	const i$1 = [], c$1 = [];
	if (0 === r$1.roughness) {
		t$1 /= 4, c$1.push([e$1 + n$1 * Math.cos(-t$1), s$1 + o$1 * Math.sin(-t$1)]);
		for (let a$2 = 0; a$2 <= 2 * Math.PI; a$2 += t$1) {
			const t$2 = [e$1 + n$1 * Math.cos(a$2), s$1 + o$1 * Math.sin(a$2)];
			i$1.push(t$2), c$1.push(t$2);
		}
		c$1.push([e$1 + n$1 * Math.cos(0), s$1 + o$1 * Math.sin(0)]), c$1.push([e$1 + n$1 * Math.cos(t$1), s$1 + o$1 * Math.sin(t$1)]);
	} else {
		const l$1 = G(.5, r$1) - Math.PI / 2;
		c$1.push([G(a$1, r$1) + e$1 + .9 * n$1 * Math.cos(l$1 - t$1), G(a$1, r$1) + s$1 + .9 * o$1 * Math.sin(l$1 - t$1)]);
		const u$1 = 2 * Math.PI + l$1 - .01;
		for (let h$2 = l$1; h$2 < u$1; h$2 += t$1) {
			const t$2 = [G(a$1, r$1) + e$1 + n$1 * Math.cos(h$2), G(a$1, r$1) + s$1 + o$1 * Math.sin(h$2)];
			i$1.push(t$2), c$1.push(t$2);
		}
		c$1.push([G(a$1, r$1) + e$1 + n$1 * Math.cos(l$1 + 2 * Math.PI + .5 * h$1), G(a$1, r$1) + s$1 + o$1 * Math.sin(l$1 + 2 * Math.PI + .5 * h$1)]), c$1.push([G(a$1, r$1) + e$1 + .98 * n$1 * Math.cos(l$1 + h$1), G(a$1, r$1) + s$1 + .98 * o$1 * Math.sin(l$1 + h$1)]), c$1.push([G(a$1, r$1) + e$1 + .9 * n$1 * Math.cos(l$1 + .5 * h$1), G(a$1, r$1) + s$1 + .9 * o$1 * Math.sin(l$1 + .5 * h$1)]);
	}
	return [c$1, i$1];
}
function V(t$1, e$1, s$1, n$1, o$1, a$1, h$1, r$1, i$1) {
	const c$1 = a$1 + G(.1, i$1), l$1 = [];
	l$1.push([G(r$1, i$1) + e$1 + .9 * n$1 * Math.cos(c$1 - t$1), G(r$1, i$1) + s$1 + .9 * o$1 * Math.sin(c$1 - t$1)]);
	for (let a$2 = c$1; a$2 <= h$1; a$2 += t$1) l$1.push([G(r$1, i$1) + e$1 + n$1 * Math.cos(a$2), G(r$1, i$1) + s$1 + o$1 * Math.sin(a$2)]);
	return l$1.push([e$1 + n$1 * Math.cos(h$1), s$1 + o$1 * Math.sin(h$1)]), l$1.push([e$1 + n$1 * Math.cos(h$1), s$1 + o$1 * Math.sin(h$1)]), q(l$1, null, i$1);
}
function Z(t$1, e$1, s$1, n$1, o$1, a$1, h$1, r$1) {
	const i$1 = [], c$1 = [r$1.maxRandomnessOffset || 1, (r$1.maxRandomnessOffset || 1) + .3];
	let l$1 = [0, 0];
	const u$1 = r$1.disableMultiStroke ? 1 : 2, p$1 = r$1.preserveVertices;
	for (let f$1 = 0; f$1 < u$1; f$1++) 0 === f$1 ? i$1.push({
		op: "move",
		data: [h$1[0], h$1[1]]
	}) : i$1.push({
		op: "move",
		data: [h$1[0] + (p$1 ? 0 : G(c$1[0], r$1)), h$1[1] + (p$1 ? 0 : G(c$1[0], r$1))]
	}), l$1 = p$1 ? [o$1, a$1] : [o$1 + G(c$1[f$1], r$1), a$1 + G(c$1[f$1], r$1)], i$1.push({
		op: "bcurveTo",
		data: [
			t$1 + G(c$1[f$1], r$1),
			e$1 + G(c$1[f$1], r$1),
			s$1 + G(c$1[f$1], r$1),
			n$1 + G(c$1[f$1], r$1),
			l$1[0],
			l$1[1]
		]
	});
	return i$1;
}
function Q(t$1) {
	return [...t$1];
}
function H(t$1, e$1 = 0) {
	const s$1 = t$1.length;
	if (s$1 < 3) throw new Error("A curve must have at least three points.");
	const n$1 = [];
	if (3 === s$1) n$1.push(Q(t$1[0]), Q(t$1[1]), Q(t$1[2]), Q(t$1[2]));
	else {
		const s$2 = [];
		s$2.push(t$1[0], t$1[0]);
		for (let e$2 = 1; e$2 < t$1.length; e$2++) s$2.push(t$1[e$2]), e$2 === t$1.length - 1 && s$2.push(t$1[e$2]);
		const o$1 = [], a$1 = 1 - e$1;
		n$1.push(Q(s$2[0]));
		for (let t$2 = 1; t$2 + 2 < s$2.length; t$2++) {
			const e$2 = s$2[t$2];
			o$1[0] = [e$2[0], e$2[1]], o$1[1] = [e$2[0] + (a$1 * s$2[t$2 + 1][0] - a$1 * s$2[t$2 - 1][0]) / 6, e$2[1] + (a$1 * s$2[t$2 + 1][1] - a$1 * s$2[t$2 - 1][1]) / 6], o$1[2] = [s$2[t$2 + 1][0] + (a$1 * s$2[t$2][0] - a$1 * s$2[t$2 + 2][0]) / 6, s$2[t$2 + 1][1] + (a$1 * s$2[t$2][1] - a$1 * s$2[t$2 + 2][1]) / 6], o$1[3] = [s$2[t$2 + 1][0], s$2[t$2 + 1][1]], n$1.push(o$1[1], o$1[2], o$1[3]);
		}
	}
	return n$1;
}
function N(t$1, e$1) {
	return Math.pow(t$1[0] - e$1[0], 2) + Math.pow(t$1[1] - e$1[1], 2);
}
function B(t$1, e$1, s$1) {
	const n$1 = N(e$1, s$1);
	if (0 === n$1) return N(t$1, e$1);
	let o$1 = ((t$1[0] - e$1[0]) * (s$1[0] - e$1[0]) + (t$1[1] - e$1[1]) * (s$1[1] - e$1[1])) / n$1;
	return o$1 = Math.max(0, Math.min(1, o$1)), N(t$1, J(e$1, s$1, o$1));
}
function J(t$1, e$1, s$1) {
	return [t$1[0] + (e$1[0] - t$1[0]) * s$1, t$1[1] + (e$1[1] - t$1[1]) * s$1];
}
function K(t$1, e$1, s$1, n$1) {
	const o$1 = n$1 || [];
	if (function(t$2, e$2) {
		const s$2 = t$2[e$2 + 0], n$2 = t$2[e$2 + 1], o$2 = t$2[e$2 + 2], a$2 = t$2[e$2 + 3];
		let h$2 = 3 * n$2[0] - 2 * s$2[0] - a$2[0];
		h$2 *= h$2;
		let r$1 = 3 * n$2[1] - 2 * s$2[1] - a$2[1];
		r$1 *= r$1;
		let i$1 = 3 * o$2[0] - 2 * a$2[0] - s$2[0];
		i$1 *= i$1;
		let c$1 = 3 * o$2[1] - 2 * a$2[1] - s$2[1];
		return c$1 *= c$1, h$2 < i$1 && (h$2 = i$1), r$1 < c$1 && (r$1 = c$1), h$2 + r$1;
	}(t$1, e$1) < s$1) {
		const s$2 = t$1[e$1 + 0];
		if (o$1.length) (a$1 = o$1[o$1.length - 1], h$1 = s$2, Math.sqrt(N(a$1, h$1))) > 1 && o$1.push(s$2);
		else o$1.push(s$2);
		o$1.push(t$1[e$1 + 3]);
	} else {
		const n$2 = .5, a$2 = t$1[e$1 + 0], h$2 = t$1[e$1 + 1], r$1 = t$1[e$1 + 2], i$1 = t$1[e$1 + 3], c$1 = J(a$2, h$2, n$2), l$1 = J(h$2, r$1, n$2), u$1 = J(r$1, i$1, n$2), p$1 = J(c$1, l$1, n$2), f$1 = J(l$1, u$1, n$2), d$1 = J(p$1, f$1, n$2);
		K([
			a$2,
			c$1,
			p$1,
			d$1
		], 0, s$1, o$1), K([
			d$1,
			f$1,
			u$1,
			i$1
		], 0, s$1, o$1);
	}
	var a$1, h$1;
	return o$1;
}
function U(t$1, e$1) {
	return X(t$1, 0, t$1.length, e$1);
}
function X(t$1, e$1, s$1, n$1, o$1) {
	const a$1 = o$1 || [], h$1 = t$1[e$1], r$1 = t$1[s$1 - 1];
	let i$1 = 0, c$1 = 1;
	for (let n$2 = e$1 + 1; n$2 < s$1 - 1; ++n$2) {
		const e$2 = B(t$1[n$2], h$1, r$1);
		e$2 > i$1 && (i$1 = e$2, c$1 = n$2);
	}
	return Math.sqrt(i$1) > n$1 ? (X(t$1, e$1, c$1 + 1, n$1, a$1), X(t$1, c$1, s$1, n$1, a$1)) : (a$1.length || a$1.push(h$1), a$1.push(r$1)), a$1;
}
function Y(t$1, e$1 = .15, s$1) {
	const n$1 = [], o$1 = (t$1.length - 1) / 3;
	for (let s$2 = 0; s$2 < o$1; s$2++) K(t$1, 3 * s$2, e$1, n$1);
	return s$1 && s$1 > 0 ? X(n$1, 0, n$1.length, s$1) : n$1;
}
var tt = "none";
var et = class {
	constructor(t$1) {
		this.defaultOptions = {
			maxRandomnessOffset: 2,
			roughness: 1,
			bowing: 1,
			stroke: "#000",
			strokeWidth: 1,
			curveTightness: 0,
			curveFitting: .95,
			curveStepCount: 9,
			fillStyle: "hachure",
			fillWeight: -1,
			hachureAngle: -41,
			hachureGap: -1,
			dashOffset: -1,
			dashGap: -1,
			zigzagOffset: -1,
			seed: 0,
			disableMultiStroke: !1,
			disableMultiStrokeFill: !1,
			preserveVertices: !1,
			fillShapeRoughnessGain: .8
		}, this.config = t$1 || {}, this.config.options && (this.defaultOptions = this._o(this.config.options));
	}
	static newSeed() {
		return Math.floor(Math.random() * 2 ** 31);
	}
	_o(t$1) {
		return t$1 ? Object.assign({}, this.defaultOptions, t$1) : this.defaultOptions;
	}
	_d(t$1, e$1, s$1) {
		return {
			shape: t$1,
			sets: e$1 || [],
			options: s$1 || this.defaultOptions
		};
	}
	line(t$1, e$1, s$1, n$1, o$1) {
		const a$1 = this._o(o$1);
		return this._d("line", [v(t$1, e$1, s$1, n$1, a$1)], a$1);
	}
	rectangle(t$1, e$1, s$1, n$1, o$1) {
		const a$1 = this._o(o$1), h$1 = [], r$1 = O(t$1, e$1, s$1, n$1, a$1);
		if (a$1.fill) {
			const o$2 = [
				[t$1, e$1],
				[t$1 + s$1, e$1],
				[t$1 + s$1, e$1 + n$1],
				[t$1, e$1 + n$1]
			];
			"solid" === a$1.fillStyle ? h$1.push(I([o$2], a$1)) : h$1.push(C([o$2], a$1));
		}
		return a$1.stroke !== tt && h$1.push(r$1), this._d("rectangle", h$1, a$1);
	}
	ellipse(t$1, e$1, s$1, n$1, o$1) {
		const a$1 = this._o(o$1), h$1 = [], r$1 = T(s$1, n$1, a$1), i$1 = D(t$1, e$1, a$1, r$1);
		if (a$1.fill) if ("solid" === a$1.fillStyle) {
			const s$2 = D(t$1, e$1, a$1, r$1).opset;
			s$2.type = "fillPath", h$1.push(s$2);
		} else h$1.push(C([i$1.estimatedPoints], a$1));
		return a$1.stroke !== tt && h$1.push(i$1.opset), this._d("ellipse", h$1, a$1);
	}
	circle(t$1, e$1, s$1, n$1) {
		const o$1 = this.ellipse(t$1, e$1, s$1, s$1, n$1);
		return o$1.shape = "circle", o$1;
	}
	linearPath(t$1, e$1) {
		const s$1 = this._o(e$1);
		return this._d("linearPath", [S(t$1, !1, s$1)], s$1);
	}
	arc(t$1, e$1, s$1, n$1, o$1, a$1, h$1 = !1, r$1) {
		const i$1 = this._o(r$1), c$1 = [], l$1 = A(t$1, e$1, s$1, n$1, o$1, a$1, h$1, !0, i$1);
		if (h$1 && i$1.fill) if ("solid" === i$1.fillStyle) {
			const h$2 = Object.assign({}, i$1);
			h$2.disableMultiStroke = !0;
			const r$2 = A(t$1, e$1, s$1, n$1, o$1, a$1, !0, !1, h$2);
			r$2.type = "fillPath", c$1.push(r$2);
		} else c$1.push(function(t$2, e$2, s$2, n$2, o$2, a$2, h$2) {
			const r$2 = t$2, i$2 = e$2;
			let c$2 = Math.abs(s$2 / 2), l$2 = Math.abs(n$2 / 2);
			c$2 += G(.01 * c$2, h$2), l$2 += G(.01 * l$2, h$2);
			let u$1 = o$2, p$1 = a$2;
			for (; u$1 < 0;) u$1 += 2 * Math.PI, p$1 += 2 * Math.PI;
			p$1 - u$1 > 2 * Math.PI && (u$1 = 0, p$1 = 2 * Math.PI);
			const f$1 = (p$1 - u$1) / h$2.curveStepCount, d$1 = [];
			for (let t$3 = u$1; t$3 <= p$1; t$3 += f$1) d$1.push([r$2 + c$2 * Math.cos(t$3), i$2 + l$2 * Math.sin(t$3)]);
			return d$1.push([r$2 + c$2 * Math.cos(p$1), i$2 + l$2 * Math.sin(p$1)]), d$1.push([r$2, i$2]), C([d$1], h$2);
		}(t$1, e$1, s$1, n$1, o$1, a$1, i$1));
		return i$1.stroke !== tt && c$1.push(l$1), this._d("arc", c$1, i$1);
	}
	curve(t$1, e$1) {
		const s$1 = this._o(e$1), n$1 = [], o$1 = L(t$1, s$1);
		if (s$1.fill && s$1.fill !== tt) if ("solid" === s$1.fillStyle) {
			const e$2 = L(t$1, Object.assign(Object.assign({}, s$1), {
				disableMultiStroke: !0,
				roughness: s$1.roughness ? s$1.roughness + s$1.fillShapeRoughnessGain : 0
			}));
			n$1.push({
				type: "fillPath",
				ops: this._mergedShape(e$2.ops)
			});
		} else {
			const e$2 = [], o$2 = t$1;
			if (o$2.length) {
				const t$2 = "number" == typeof o$2[0][0] ? [o$2] : o$2;
				for (const n$2 of t$2) n$2.length < 3 ? e$2.push(...n$2) : 3 === n$2.length ? e$2.push(...Y(H([
					n$2[0],
					n$2[0],
					n$2[1],
					n$2[2]
				]), 10, (1 + s$1.roughness) / 2)) : e$2.push(...Y(H(n$2), 10, (1 + s$1.roughness) / 2));
			}
			e$2.length && n$1.push(C([e$2], s$1));
		}
		return s$1.stroke !== tt && n$1.push(o$1), this._d("curve", n$1, s$1);
	}
	polygon(t$1, e$1) {
		const s$1 = this._o(e$1), n$1 = [], o$1 = S(t$1, !0, s$1);
		return s$1.fill && ("solid" === s$1.fillStyle ? n$1.push(I([t$1], s$1)) : n$1.push(C([t$1], s$1))), s$1.stroke !== tt && n$1.push(o$1), this._d("polygon", n$1, s$1);
	}
	path(t$1, e$1) {
		const s$1 = this._o(e$1), n$1 = [];
		if (!t$1) return this._d("path", n$1, s$1);
		t$1 = (t$1 || "").replace(/\n/g, " ").replace(/(-\s)/g, "-").replace("/(ss)/g", " ");
		const o$1 = s$1.fill && "transparent" !== s$1.fill && s$1.fill !== tt, a$1 = s$1.stroke !== tt, h$1 = !!(s$1.simplification && s$1.simplification < 1), r$1 = function(t$2, e$2, s$2) {
			const n$2 = m(y(b(t$2))), o$2 = [];
			let a$2 = [], h$2 = [0, 0], r$2 = [];
			const i$2 = () => {
				r$2.length >= 4 && a$2.push(...Y(r$2, e$2)), r$2 = [];
			}, c$1 = () => {
				i$2(), a$2.length && (o$2.push(a$2), a$2 = []);
			};
			for (const { key: t$3, data: e$3 } of n$2) switch (t$3) {
				case "M":
					c$1(), h$2 = [e$3[0], e$3[1]], a$2.push(h$2);
					break;
				case "L":
					i$2(), a$2.push([e$3[0], e$3[1]]);
					break;
				case "C":
					if (!r$2.length) {
						const t$4 = a$2.length ? a$2[a$2.length - 1] : h$2;
						r$2.push([t$4[0], t$4[1]]);
					}
					r$2.push([e$3[0], e$3[1]]), r$2.push([e$3[2], e$3[3]]), r$2.push([e$3[4], e$3[5]]);
					break;
				case "Z": i$2(), a$2.push([h$2[0], h$2[1]]);
			}
			if (c$1(), !s$2) return o$2;
			const l$1 = [];
			for (const t$3 of o$2) {
				const e$3 = U(t$3, s$2);
				e$3.length && l$1.push(e$3);
			}
			return l$1;
		}(t$1, 1, h$1 ? 4 - 4 * (s$1.simplification || 1) : (1 + s$1.roughness) / 2), i$1 = _(t$1, s$1);
		if (o$1) if ("solid" === s$1.fillStyle) if (1 === r$1.length) {
			const e$2 = _(t$1, Object.assign(Object.assign({}, s$1), {
				disableMultiStroke: !0,
				roughness: s$1.roughness ? s$1.roughness + s$1.fillShapeRoughnessGain : 0
			}));
			n$1.push({
				type: "fillPath",
				ops: this._mergedShape(e$2.ops)
			});
		} else n$1.push(I(r$1, s$1));
		else n$1.push(C(r$1, s$1));
		return a$1 && (h$1 ? r$1.forEach(((t$2) => {
			n$1.push(S(t$2, !1, s$1));
		})) : n$1.push(i$1)), this._d("path", n$1, s$1);
	}
	opsToPath(t$1, e$1) {
		let s$1 = "";
		for (const n$1 of t$1.ops) {
			const t$2 = "number" == typeof e$1 && e$1 >= 0 ? n$1.data.map(((t$3) => +t$3.toFixed(e$1))) : n$1.data;
			switch (n$1.op) {
				case "move":
					s$1 += `M${t$2[0]} ${t$2[1]} `;
					break;
				case "bcurveTo":
					s$1 += `C${t$2[0]} ${t$2[1]}, ${t$2[2]} ${t$2[3]}, ${t$2[4]} ${t$2[5]} `;
					break;
				case "lineTo": s$1 += `L${t$2[0]} ${t$2[1]} `;
			}
		}
		return s$1.trim();
	}
	toPaths(t$1) {
		const e$1 = t$1.sets || [], s$1 = t$1.options || this.defaultOptions, n$1 = [];
		for (const t$2 of e$1) {
			let e$2 = null;
			switch (t$2.type) {
				case "path":
					e$2 = {
						d: this.opsToPath(t$2),
						stroke: s$1.stroke,
						strokeWidth: s$1.strokeWidth,
						fill: tt
					};
					break;
				case "fillPath":
					e$2 = {
						d: this.opsToPath(t$2),
						stroke: tt,
						strokeWidth: 0,
						fill: s$1.fill || tt
					};
					break;
				case "fillSketch": e$2 = this.fillSketch(t$2, s$1);
			}
			e$2 && n$1.push(e$2);
		}
		return n$1;
	}
	fillSketch(t$1, e$1) {
		let s$1 = e$1.fillWeight;
		return s$1 < 0 && (s$1 = e$1.strokeWidth / 2), {
			d: this.opsToPath(t$1),
			stroke: e$1.fill || tt,
			strokeWidth: s$1,
			fill: tt
		};
	}
	_mergedShape(t$1) {
		return t$1.filter(((t$2, e$1) => 0 === e$1 || "move" !== t$2.op));
	}
};
var st = class {
	constructor(t$1, e$1) {
		this.canvas = t$1, this.ctx = this.canvas.getContext("2d"), this.gen = new et(e$1);
	}
	draw(t$1) {
		const e$1 = t$1.sets || [], s$1 = t$1.options || this.getDefaultOptions(), n$1 = this.ctx, o$1 = t$1.options.fixedDecimalPlaceDigits;
		for (const a$1 of e$1) switch (a$1.type) {
			case "path":
				n$1.save(), n$1.strokeStyle = "none" === s$1.stroke ? "transparent" : s$1.stroke, n$1.lineWidth = s$1.strokeWidth, s$1.strokeLineDash && n$1.setLineDash(s$1.strokeLineDash), s$1.strokeLineDashOffset && (n$1.lineDashOffset = s$1.strokeLineDashOffset), this._drawToContext(n$1, a$1, o$1), n$1.restore();
				break;
			case "fillPath": {
				n$1.save(), n$1.fillStyle = s$1.fill || "";
				const e$2 = "curve" === t$1.shape || "polygon" === t$1.shape || "path" === t$1.shape ? "evenodd" : "nonzero";
				this._drawToContext(n$1, a$1, o$1, e$2), n$1.restore();
				break;
			}
			case "fillSketch": this.fillSketch(n$1, a$1, s$1);
		}
	}
	fillSketch(t$1, e$1, s$1) {
		let n$1 = s$1.fillWeight;
		n$1 < 0 && (n$1 = s$1.strokeWidth / 2), t$1.save(), s$1.fillLineDash && t$1.setLineDash(s$1.fillLineDash), s$1.fillLineDashOffset && (t$1.lineDashOffset = s$1.fillLineDashOffset), t$1.strokeStyle = s$1.fill || "", t$1.lineWidth = n$1, this._drawToContext(t$1, e$1, s$1.fixedDecimalPlaceDigits), t$1.restore();
	}
	_drawToContext(t$1, e$1, s$1, n$1 = "nonzero") {
		t$1.beginPath();
		for (const n$2 of e$1.ops) {
			const e$2 = "number" == typeof s$1 && s$1 >= 0 ? n$2.data.map(((t$2) => +t$2.toFixed(s$1))) : n$2.data;
			switch (n$2.op) {
				case "move":
					t$1.moveTo(e$2[0], e$2[1]);
					break;
				case "bcurveTo":
					t$1.bezierCurveTo(e$2[0], e$2[1], e$2[2], e$2[3], e$2[4], e$2[5]);
					break;
				case "lineTo": t$1.lineTo(e$2[0], e$2[1]);
			}
		}
		"fillPath" === e$1.type ? t$1.fill(n$1) : t$1.stroke();
	}
	get generator() {
		return this.gen;
	}
	getDefaultOptions() {
		return this.gen.defaultOptions;
	}
	line(t$1, e$1, s$1, n$1, o$1) {
		const a$1 = this.gen.line(t$1, e$1, s$1, n$1, o$1);
		return this.draw(a$1), a$1;
	}
	rectangle(t$1, e$1, s$1, n$1, o$1) {
		const a$1 = this.gen.rectangle(t$1, e$1, s$1, n$1, o$1);
		return this.draw(a$1), a$1;
	}
	ellipse(t$1, e$1, s$1, n$1, o$1) {
		const a$1 = this.gen.ellipse(t$1, e$1, s$1, n$1, o$1);
		return this.draw(a$1), a$1;
	}
	circle(t$1, e$1, s$1, n$1) {
		const o$1 = this.gen.circle(t$1, e$1, s$1, n$1);
		return this.draw(o$1), o$1;
	}
	linearPath(t$1, e$1) {
		const s$1 = this.gen.linearPath(t$1, e$1);
		return this.draw(s$1), s$1;
	}
	polygon(t$1, e$1) {
		const s$1 = this.gen.polygon(t$1, e$1);
		return this.draw(s$1), s$1;
	}
	arc(t$1, e$1, s$1, n$1, o$1, a$1, h$1 = !1, r$1) {
		const i$1 = this.gen.arc(t$1, e$1, s$1, n$1, o$1, a$1, h$1, r$1);
		return this.draw(i$1), i$1;
	}
	curve(t$1, e$1) {
		const s$1 = this.gen.curve(t$1, e$1);
		return this.draw(s$1), s$1;
	}
	path(t$1, e$1) {
		const s$1 = this.gen.path(t$1, e$1);
		return this.draw(s$1), s$1;
	}
};
var nt = "http://www.w3.org/2000/svg";
var ot = class {
	constructor(t$1, e$1) {
		this.svg = t$1, this.gen = new et(e$1);
	}
	draw(t$1) {
		const e$1 = t$1.sets || [], s$1 = t$1.options || this.getDefaultOptions(), n$1 = this.svg.ownerDocument || window.document, o$1 = n$1.createElementNS(nt, "g"), a$1 = t$1.options.fixedDecimalPlaceDigits;
		for (const h$1 of e$1) {
			let e$2 = null;
			switch (h$1.type) {
				case "path":
					e$2 = n$1.createElementNS(nt, "path"), e$2.setAttribute("d", this.opsToPath(h$1, a$1)), e$2.setAttribute("stroke", s$1.stroke), e$2.setAttribute("stroke-width", s$1.strokeWidth + ""), e$2.setAttribute("fill", "none"), s$1.strokeLineDash && e$2.setAttribute("stroke-dasharray", s$1.strokeLineDash.join(" ").trim()), s$1.strokeLineDashOffset && e$2.setAttribute("stroke-dashoffset", `${s$1.strokeLineDashOffset}`);
					break;
				case "fillPath":
					e$2 = n$1.createElementNS(nt, "path"), e$2.setAttribute("d", this.opsToPath(h$1, a$1)), e$2.setAttribute("stroke", "none"), e$2.setAttribute("stroke-width", "0"), e$2.setAttribute("fill", s$1.fill || ""), "curve" !== t$1.shape && "polygon" !== t$1.shape || e$2.setAttribute("fill-rule", "evenodd");
					break;
				case "fillSketch": e$2 = this.fillSketch(n$1, h$1, s$1);
			}
			e$2 && o$1.appendChild(e$2);
		}
		return o$1;
	}
	fillSketch(t$1, e$1, s$1) {
		let n$1 = s$1.fillWeight;
		n$1 < 0 && (n$1 = s$1.strokeWidth / 2);
		const o$1 = t$1.createElementNS(nt, "path");
		return o$1.setAttribute("d", this.opsToPath(e$1, s$1.fixedDecimalPlaceDigits)), o$1.setAttribute("stroke", s$1.fill || ""), o$1.setAttribute("stroke-width", n$1 + ""), o$1.setAttribute("fill", "none"), s$1.fillLineDash && o$1.setAttribute("stroke-dasharray", s$1.fillLineDash.join(" ").trim()), s$1.fillLineDashOffset && o$1.setAttribute("stroke-dashoffset", `${s$1.fillLineDashOffset}`), o$1;
	}
	get generator() {
		return this.gen;
	}
	getDefaultOptions() {
		return this.gen.defaultOptions;
	}
	opsToPath(t$1, e$1) {
		return this.gen.opsToPath(t$1, e$1);
	}
	line(t$1, e$1, s$1, n$1, o$1) {
		const a$1 = this.gen.line(t$1, e$1, s$1, n$1, o$1);
		return this.draw(a$1);
	}
	rectangle(t$1, e$1, s$1, n$1, o$1) {
		const a$1 = this.gen.rectangle(t$1, e$1, s$1, n$1, o$1);
		return this.draw(a$1);
	}
	ellipse(t$1, e$1, s$1, n$1, o$1) {
		const a$1 = this.gen.ellipse(t$1, e$1, s$1, n$1, o$1);
		return this.draw(a$1);
	}
	circle(t$1, e$1, s$1, n$1) {
		const o$1 = this.gen.circle(t$1, e$1, s$1, n$1);
		return this.draw(o$1);
	}
	linearPath(t$1, e$1) {
		const s$1 = this.gen.linearPath(t$1, e$1);
		return this.draw(s$1);
	}
	polygon(t$1, e$1) {
		const s$1 = this.gen.polygon(t$1, e$1);
		return this.draw(s$1);
	}
	arc(t$1, e$1, s$1, n$1, o$1, a$1, h$1 = !1, r$1) {
		const i$1 = this.gen.arc(t$1, e$1, s$1, n$1, o$1, a$1, h$1, r$1);
		return this.draw(i$1);
	}
	curve(t$1, e$1) {
		const s$1 = this.gen.curve(t$1, e$1);
		return this.draw(s$1);
	}
	path(t$1, e$1) {
		const s$1 = this.gen.path(t$1, e$1);
		return this.draw(s$1);
	}
};
var at = {
	canvas: (t$1, e$1) => new st(t$1, e$1),
	svg: (t$1, e$1) => new ot(t$1, e$1),
	generator: (t$1) => new et(t$1),
	newSeed: () => et.newSeed()
};
export { at as t };
