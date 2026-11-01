// Ferryx browser-agent page observer, evaluated as `<this file>(<op json>)` in a tab.
//
// Isolation: in a Chromium engine tab the host runs it in an isolated world, whose globals and
// DOM wrappers the page's scripts cannot see or change. In a system webview tab it shares the
// page's world, so every DOM and builtin function it calls comes from a fresh about:blank frame
// made for the call: page code that overwrites document.querySelector, Element.prototype
// methods, JSON or Array.prototype cannot steer it. The page could still reach the ref store
// there; see the known limit below.
//
// Refs name node identity: `<document token>.<n>`, assigned once per node through a WeakMap and
// never reused. A replaced node gets a new ref and the old ref fails as stale; a new document
// has a new token, so no old ref resolves there.
//
// Values are never read: no input, textarea or password value reaches the result.
//
// Before acting on a ref the script checks that the node still has the tag, role and name the
// observe showed (`op.expect`, supplied by the host from its own record of that observe).
//
// Known limit (webview tabs only): reaching the clean realm uses the page's own
// Reflect.apply, createElement, setAttribute and appendChild once per call, and a page that
// hooks the frame's synchronous load or mutation events can reach the new realm before this
// script reads it; the ref store is a non-enumerable window property under a per-run random
// key, which page code can still find. Such a page can steer the call, including that check.
// Chromium engine tabs have neither limit.
(function (input) {
  'use strict';
  // The host's op, copied by own properties onto a null-prototype object before anything
  // reads it: a getter the page put on Object.prototype never answers a missing field.
  var op = Object.create(null);
  var OP_FIELDS = ['op', 'key', 'isolated', 'max', 'ref', 'expect', 'action', 'check'];
  for (var f = 0; f < OP_FIELDS.length; f += 1) {
    var field = Object.getOwnPropertyDescriptor(input, OP_FIELDS[f]);
    op[OP_FIELDS[f]] = field ? field.value : null;
  }
  var frame = null;
  var P = window;
  if (!op.isolated) {
    try {
      frame = Reflect.apply(Document.prototype.createElement, document, ['iframe']);
      Reflect.apply(Element.prototype.setAttribute, frame, ['style', 'display:none']);
      Reflect.apply(Node.prototype.appendChild, document.documentElement, [frame]);
      var realm = frame.contentWindow;
      if (realm && realm.Reflect && realm.Object && realm.Element && realm.WeakMap) P = realm;
    } catch (error) {
      frame = null;
    }
  }
  var apply = P.Reflect.apply;
  var describe = P.Object.getOwnPropertyDescriptor;
  function part(iface, name, kind) {
    var proto = P[iface] && P[iface].prototype;
    var d = proto && describe(proto, name);
    return d ? d[kind] : undefined;
  }
  function method(iface, name) { return part(iface, name, 'value'); }
  function getter(iface, name) { return part(iface, name, 'get'); }
  function setter(iface, name) { return part(iface, name, 'set'); }
  function call(fn, self, args) { return apply(fn, self, args || []); }
  // A pristine accessor where the engine exposes one, else a plain read.
  function read(get, self, name) { return get ? call(get, self) : self[name]; }
  var defineOwn = P.Object.defineProperty;
  // Own-property writes only: a setter the page put on Array.prototype or Object.prototype
  // never runs.
  function push(list, value) {
    defineOwn(list, list.length, { value: value, writable: true, enumerable: true, configurable: true });
  }
  function own(object, key) {
    var d = object ? describe(object, key) : undefined;
    return d ? d.value : undefined;
  }
  // A null-prototype copy of a literal: the browser reads options dictionaries with plain
  // property gets, which would otherwise reach getters the page put on Object.prototype.
  function dict(source) {
    var out = P.Object.create(null);
    var keys = P.Object.keys(source);
    for (var i = 0; i < keys.length; i += 1) {
      defineOwn(out, keys[i], { value: source[keys[i]], writable: true, enumerable: true, configurable: true });
    }
    return out;
  }

  var M = {
    qsa: method('Document', 'querySelectorAll'), fragQsa: method('DocumentFragment', 'querySelectorAll'),
    listLength: getter('NodeList', 'length'), attr: method('Element', 'getAttribute'),
    hasAttr: method('Element', 'hasAttribute'), tag: getter('Element', 'tagName'),
    shadow: getter('Element', 'shadowRoot'), parent: getter('Node', 'parentElement'),
    rect: method('Element', 'getBoundingClientRect'), visible: method('Element', 'checkVisibility'),
    scrollIntoView: method('Element', 'scrollIntoView'), scrollBy: method('Element', 'scrollBy'),
    text: getter('Node', 'textContent'), setText: setter('Node', 'textContent'),
    connected: getter('Node', 'isConnected'), contains: method('Node', 'contains'),
    root: method('Node', 'getRootNode'), byId: method('Document', 'getElementById'),
    fragById: method('DocumentFragment', 'getElementById'), fromPoint: method('Document', 'elementFromPoint'),
    shadowFromPoint: method('ShadowRoot', 'elementFromPoint'), remove: method('Element', 'remove'),
    title: getter('Document', 'title'), scrolling: getter('Document', 'scrollingElement'),
    exec: method('Document', 'execCommand'), dispatch: method('EventTarget', 'dispatchEvent'),
    click: method('HTMLElement', 'click'), focus: method('HTMLElement', 'focus'),
    inputType: getter('HTMLInputElement', 'type'), checked: getter('HTMLInputElement', 'checked'),
    options: getter('HTMLSelectElement', 'options'), optionsLength: getter('HTMLOptionsCollection', 'length'),
    optionValue: getter('HTMLOptionElement', 'value'), optionText: getter('HTMLOptionElement', 'text'),
    optionSelected: setter('HTMLOptionElement', 'selected'), editable: getter('HTMLElement', 'isContentEditable'),
    mapGet: method('Map', 'get'), mapSet: method('Map', 'set'), mapDelete: method('Map', 'delete'),
    mapKeys: method('Map', 'keys'), weakGet: method('WeakMap', 'get'), weakSet: method('WeakMap', 'set'),
    deref: method('WeakRef', 'deref'), trim: method('String', 'trim'), replace: method('String', 'replace'),
    slice: method('String', 'slice'), lower: method('String', 'toLowerCase'), split: method('String', 'split'),
    indexOf: method('String', 'indexOf'), toString36: method('Number', 'toString'),
    charCode: method('String', 'charCodeAt'), includes: method('String', 'includes'),
    innerText: getter('HTMLElement', 'innerText'), body: getter('Document', 'body'),
    readyState: getter('Document', 'readyState'),
    rectLeft: getter('DOMRectReadOnly', 'left'), rectTop: getter('DOMRectReadOnly', 'top'),
    rectWidth: getter('DOMRectReadOnly', 'width'), rectHeight: getter('DOMRectReadOnly', 'height'),
  };
  var VALUE_SETTERS = {
    input: setter('HTMLInputElement', 'value'), textarea: setter('HTMLTextAreaElement', 'value'),
    select: setter('HTMLSelectElement', 'value'),
  };
  var VALUE_GETTERS = { input: getter('HTMLInputElement', 'value'), textarea: getter('HTMLTextAreaElement', 'value') };
  var LABELS = { input: 'HTMLInputElement', select: 'HTMLSelectElement', textarea: 'HTMLTextAreaElement', button: 'HTMLButtonElement' };
  var SPACES = new P.RegExp('\\s+', 'g');
  var CANDIDATES = 'a[href],button,input,select,textarea,summary,[role],[contenteditable=""],[contenteditable="true"],[tabindex]';
  var FRAMES = 'iframe,frame,object,embed';

  function attr(el, name) { return call(M.attr, el, [name]); }
  function tagOf(el) { return call(M.lower, call(M.tag, el)); }
  function clean(value, max) {
    if (value === null || value === undefined) return '';
    var text = call(M.trim, call(M.replace, '' + value, [SPACES, ' ']));
    return call(M.slice, text, [0, max || 160]);
  }
  function each(list, visit) {
    var length = call(M.listLength, list);
    for (var i = 0; i < length; i += 1) if (visit(list[i]) === false) return false;
    return true;
  }

  function storeFor(create) {
    var held = describe(window, op.key);
    if (held) return held.value;
    if (!create) return null;
    var words = new P.Uint32Array(2);
    var token;
    try {
      P.crypto.getRandomValues(words);
      token = call(M.toString36, words[0], [36]) + call(M.toString36, words[1], [36]);
    } catch (error) {
      token = call(M.toString36, P.Math.floor(P.Math.random() * 4294967296), [36]);
    }
    var store = P.Object.create(null);
    store.doc = token;
    store.next = 1;
    store.ids = new P.WeakMap();
    store.refs = new P.Map();
    P.Object.defineProperty(window, op.key, dict({ value: store }));
    return store;
  }
  function hold(el) { return P.WeakRef ? new P.WeakRef(el) : el; }
  function release(held) { return P.WeakRef ? call(M.deref, held) : held; }
  function refFor(store, el) {
    var ref = call(M.weakGet, store.ids, [el]);
    if (ref === undefined) {
      ref = store.doc + '.' + store.next;
      store.next += 1;
      call(M.weakSet, store.ids, [el, ref]);
    }
    call(M.mapSet, store.refs, [ref, hold(el)]);
    return ref;
  }

  function hiddenSelf(el) {
    if (call(M.hasAttr, el, ['hidden']) || attr(el, 'aria-hidden') === 'true') return true;
    var style = null;
    try { style = call(P.getComputedStyle, P, [el]); } catch (error) { style = null; }
    // Some engines refuse another window's element; the page's own function is the fallback
    // (it only decides which elements are listed).
    if (!style) try { style = window.getComputedStyle(el); } catch (error) { style = null; }
    return !!style && (style.display === 'none' || style.visibility === 'hidden');
  }
  function hidden(el) {
    if (M.visible) {
      try { return !call(M.visible, el, [dict({ visibilityProperty: true })]); } catch (error) { /* walk below */ }
    }
    for (var node = el; node; node = call(M.parent, node)) if (hiddenSelf(node)) return true;
    return false;
  }

  function roleOf(el, tag) {
    var explicit = attr(el, 'role');
    if (explicit) return call(M.split, call(M.trim, explicit), [' '])[0];
    if (tag === 'a') return 'link';
    if (tag === 'button' || tag === 'summary') return 'button';
    if (tag === 'textarea') return 'textbox';
    if (tag === 'select') return call(M.hasAttr, el, ['multiple']) ? 'listbox' : 'combobox';
    if (tag === 'input') {
      var type = call(M.inputType, el);
      if (type === 'checkbox' || type === 'radio') return type;
      if (type === 'submit' || type === 'button' || type === 'reset' || type === 'image') return 'button';
      if (type === 'range') return 'slider';
      if (type === 'search') return 'searchbox';
      return 'textbox';
    }
    if (M.editable && call(M.editable, el)) return 'textbox';
    return tag;
  }
  function textById(el, ids) {
    var root = call(M.root, el);
    var byId = root === document ? M.byId : M.fragById;
    var parts = call(M.split, ids, [SPACES]);
    var out = '';
    for (var i = 0; i < parts.length; i += 1) {
      var target = parts[i] && byId ? call(byId, root, [parts[i]]) : null;
      if (target) out += ' ' + call(M.text, target);
    }
    return clean(out);
  }
  function nameOf(el, tag) {
    var by = attr(el, 'aria-labelledby');
    var name = by ? textById(el, by) : '';
    if (!name) name = clean(attr(el, 'aria-label'));
    if (!name && LABELS[tag]) {
      var labels = null;
      try { labels = call(getter(LABELS[tag], 'labels'), el); } catch (error) { labels = null; }
      var joined = '';
      if (labels) each(labels, function (label) { joined += ' ' + call(M.text, label); });
      name = clean(joined);
    }
    if (!name && tag === 'input') {
      var type = call(M.inputType, el);
      if (type === 'submit' || type === 'button' || type === 'reset') name = clean(attr(el, 'value')) || type;
      if (type === 'image') name = clean(attr(el, 'alt'));
    }
    if (!name) name = clean(attr(el, 'title'));
    if (!name) name = clean(attr(el, 'placeholder'));
    if (!name && tag !== 'input' && tag !== 'textarea' && tag !== 'select') name = clean(call(M.text, el));
    return name;
  }
  function checkedOf(el, tag, role) {
    if (tag === 'input' && (role === 'checkbox' || role === 'radio')) return !!call(M.checked, el);
    var aria = attr(el, 'aria-checked');
    if (aria === 'true') return true;
    if (aria === 'false') return false;
    return null;
  }
  // What the host checks before acting on a ref. A text box's name can come from its own
  // content (contenteditable), so it is left out for those roles.
  function fingerprint(el) {
    var tag = tagOf(el);
    var role = roleOf(el, tag);
    var name = role === 'textbox' || role === 'searchbox' ? '' : nameOf(el, tag);
    return tag + '\n' + role + '\n' + name;
  }

  function observe(store) {
    var elements = [];
    var frames = [];
    var truncated = false;
    var max = op.max || 1000;
    var roots = [];
    push(roots, document);
    for (var r = 0; r < roots.length; r += 1) {
      var root = roots[r];
      var qsa = root === document ? M.qsa : M.fragQsa;
      each(call(qsa, root, ['*']), function (el) {
        var shadow = call(M.shadow, el);
        if (shadow) push(roots, shadow);
      });
      each(call(qsa, root, [CANDIDATES]), function (el) {
        var tag = tagOf(el);
        if ((tag === 'input' && call(M.inputType, el) === 'hidden') || hidden(el)) return true;
        if (elements.length >= max) { truncated = true; return false; }
        var role = roleOf(el, tag);
        push(elements, {
          reference: refFor(store, el), role: role, name: nameOf(el, tag), tagName: tag,
          disabled: call(M.hasAttr, el, ['disabled']) || attr(el, 'aria-disabled') === 'true',
          checked: checkedOf(el, tag, role),
        });
        return true;
      });
      each(call(qsa, root, [FRAMES]), function (el) {
        if (el === frame) return true;
        var tag = tagOf(el);
        push(frames, { kind: tag, title: clean(attr(el, 'title')), src: clean(attr(el, tag === 'object' ? 'data' : 'src'), 2048) });
      });
    }
    var keys = call(M.mapKeys, store.refs);
    var stale = [];
    for (var step = keys.next(); !step.done; step = keys.next()) {
      var node = release(call(M.mapGet, store.refs, [step.value]));
      if (!node || !call(M.connected, node)) push(stale, step.value);
    }
    for (var s = 0; s < stale.length; s += 1) call(M.mapDelete, store.refs, [stale[s]]);
    return {
      ok: true, url: '' + location.href, title: clean(call(M.title, document), 1024), doc: store.doc,
      elements: elements, frames: frames, truncated: truncated,
    };
  }

  function fail(code, message) { return { ok: false, code: code, message: message }; }
  function resolve(store) {
    var ref = '' + op.ref;
    var dot = call(M.indexOf, ref, ['.']);
    if (dot < 0 || call(M.slice, ref, [0, dot]) !== store.doc) {
      return fail('stale', 'the ref belongs to another document; observe again');
    }
    var held = call(M.mapGet, store.refs, [ref]);
    if (held === undefined) return fail('unknown', 'no element has this ref');
    var el = release(held);
    if (!el || !call(M.connected, el)) return fail('stale', 'the element was removed or replaced; observe again');
    if (typeof op.expect === 'string' && fingerprint(el) !== op.expect) {
      return fail('changed', 'the element changed since the observe; observe again');
    }
    return { ok: true, el: el };
  }

  function fire(el, type, kind, extra) {
    var Ctor = P[kind] || P.Event;
    var quiet = own(extra, 'quiet') === true;
    var options = dict({ bubbles: !quiet, cancelable: !quiet, composed: true });
    var names = ['detail', 'key', 'data'];
    for (var i = 0; i < names.length; i += 1) {
      var value = own(extra, names[i]);
      if (value !== undefined) defineOwn(options, names[i], { value: value, enumerable: true });
    }
    return call(M.dispatch, el, [new Ctor(type, options)]);
  }
  function reveal(el) {
    if (M.scrollIntoView) call(M.scrollIntoView, el, [dict({ behavior: 'instant', block: 'center', inline: 'center' })]);
  }
  function focus(el) { if (M.focus) try { call(M.focus, el); } catch (error) { /* not focusable */ } }
  // The pristine HTMLElement click (the page's own may be replaced); an SVG or MathML element
  // has none, so it gets a dispatched click event.
  function activate(el) {
    try {
      call(M.click, el);
    } catch (error) {
      fire(el, 'click', 'MouseEvent');
    }
  }
  function setValue(el, tag, value) {
    var set = VALUE_SETTERS[tag];
    if (set) {
      call(set, el, [value]);
    } else if (M.editable && call(M.editable, el)) {
      call(M.setText, el, [value]);
    } else {
      return false;
    }
    fire(el, 'input', 'InputEvent');
    fire(el, 'change', 'Event');
    return true;
  }
  function typeInto(el, tag, text) {
    focus(el);
    var set = tag === 'select' ? undefined : VALUE_SETTERS[tag];
    var current = set ? '' + call(VALUE_GETTERS[tag], el) : '';
    for (var i = 0; i < text.length; i += 1) {
      // One key per code point: a surrogate pair stays together.
      var code = call(M.charCode, text, [i]);
      var ch = call(M.slice, text, [i, code >= 0xd800 && code <= 0xdbff ? i + 2 : i + 1]);
      i += ch.length - 1;
      fire(el, 'keydown', 'KeyboardEvent', { key: ch });
      fire(el, 'keypress', 'KeyboardEvent', { key: ch });
      if (set) {
        current += ch;
        call(set, el, [current]);
        fire(el, 'input', 'InputEvent', { data: ch });
      } else if (M.editable && call(M.editable, el) && M.exec) {
        call(M.exec, document, ['insertText', false, ch]);
      }
      fire(el, 'keyup', 'KeyboardEvent', { key: ch });
    }
    if (set) fire(el, 'change', 'Event');
  }
  function select(el, value) {
    var options = call(M.options, el);
    var length = read(M.optionsLength, options, 'length');
    for (var i = 0; i < length; i += 1) {
      var option = options[i];
      if (call(M.optionValue, option) === value || clean(call(M.optionText, option)) === value) {
        call(M.optionSelected, option, [true]);
        fire(el, 'input', 'InputEvent');
        fire(el, 'change', 'Event');
        return true;
      }
    }
    return false;
  }
  function point(el) {
    reveal(el);
    var rect = call(M.rect, el);
    var width = read(M.rectWidth, rect, 'width');
    var height = read(M.rectHeight, rect, 'height');
    if (!(width > 0 && height > 0) || !M.fromPoint) return null;
    var x = read(M.rectLeft, rect, 'left') + width / 2;
    var y = read(M.rectTop, rect, 'top') + height / 2;
    var root = call(M.root, el);
    var hit = root !== document && M.shadowFromPoint
      ? call(M.shadowFromPoint, root, [x, y])
      : call(M.fromPoint, document, [x, y]);
    if (!hit || !(hit === el || call(M.contains, el, [hit]))) return null;
    return { x: x, y: y };
  }

  function act(store) {
    var found = resolve(store);
    if (!found.ok) return found;
    var el = found.el;
    var tag = tagOf(el);
    var action = op.action;
    function arg(name) { return own(action, name); }
    switch (arg('type')) {
      case 'point': return { ok: true, point: point(el) };
      case 'click': reveal(el); activate(el); break;
      case 'dblclick': reveal(el); fire(el, 'dblclick', 'MouseEvent', { detail: 2 }); break;
      case 'hover':
        fire(el, 'mouseover', 'MouseEvent');
        fire(el, 'mouseenter', 'MouseEvent', { quiet: true });
        fire(el, 'mousemove', 'MouseEvent');
        break;
      case 'focus': focus(el); break;
      case 'fill':
        focus(el);
        if (!setValue(el, tag, '' + arg('value'))) return fail('unsupported', 'the element takes no text');
        break;
      case 'check':
      case 'uncheck': {
        var want = arg('type') === 'check';
        var role = roleOf(el, tag);
        var state = checkedOf(el, tag, role);
        if (state === null) return fail('unsupported', 'the element has no checked state');
        if (state !== want) activate(el);
        break;
      }
      case 'scrollIntoView': reveal(el); break;
      case 'select':
        if (tag !== 'select') return fail('unsupported', 'the element is not a select');
        if (!select(el, '' + arg('value'))) return fail('noOption', 'no option has this value or text');
        break;
      case 'scroll':
        if (M.scrollBy) call(M.scrollBy, el, [dict({ left: arg('x') || 0, top: arg('y') || 0, behavior: 'instant' })]);
        break;
      case 'type': typeInto(el, tag, '' + arg('text')); break;
      default: return fail('unsupported', 'unknown action');
    }
    return { ok: true };
  }

  // Whether the page now shows the state an act's postcondition names. Reads only: it never
  // creates the ref store.
  function check() {
    var value = '' + own(op.check, 'value');
    switch (own(op.check, 'kind')) {
      case 'text': {
        var body = call(M.body, document);
        var text = body ? (M.innerText ? call(M.innerText, body) : call(M.text, body)) : '';
        return { ok: true, matched: call(M.includes, '' + (text || ''), [value]) };
      }
      case 'selector':
        try {
          return { ok: true, matched: call(M.listLength, call(M.qsa, document, [value])) > 0 };
        } catch (error) {
          return fail('selector', 'the postcondition selector is invalid');
        }
      case 'url': return { ok: true, matched: call(M.includes, '' + location.href, [value]) };
      case 'load': {
        var state = call(M.readyState, document);
        return { ok: true, matched: value === 'complete' ? state === 'complete' : state !== 'loading' };
      }
      default: return fail('unsupported', 'unknown postcondition');
    }
  }

  try {
    if (op.op === 'check') return check();
    if (op.op === 'observe') return observe(storeFor(true));
    var store = storeFor(false);
    if (!store) return fail('stale', 'this document has not been observed; observe again');
    return act(store);
  } finally {
    if (frame && M.remove) try { call(M.remove, frame); } catch (error) { /* page removed it */ }
  }
})
