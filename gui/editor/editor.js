// The source of gui/assets/editor.js, built with `npm run gui:editor`: CodeMirror 6 with the
// languages the GUI shows, bundled so that the window loads nothing from the network.
import { EditorView, basicSetup } from "codemirror";
import { EditorState, StateEffect, StateField } from "@codemirror/state";
import { Decoration, keymap } from "@codemirror/view";
import { indentWithTab } from "@codemirror/commands";
import {
  HighlightStyle,
  StreamLanguage,
  syntaxHighlighting,
} from "@codemirror/language";
import { tags } from "@lezer/highlight";
import { markdown } from "@codemirror/lang-markdown";
import { yaml } from "@codemirror/lang-yaml";
import { rust } from "@codemirror/lang-rust";
import { javascript } from "@codemirror/lang-javascript";
import { python } from "@codemirror/lang-python";
import { php } from "@codemirror/lang-php";
import { json } from "@codemirror/lang-json";
import { dart } from "@codemirror/legacy-modes/mode/clike";

const languages = {
  markdown: () => markdown(),
  yaml: () => yaml(),
  rust: () => rust(),
  typescript: () => javascript({ typescript: true, jsx: true }),
  javascript: () => javascript({ jsx: true }),
  python: () => python(),
  php: () => php({ plain: true }),
  json: () => json(),
  dart: () => StreamLanguage.define(dart),
};

// Colours come from hone's tokens, so both themes and the theme switch apply.
const highlight = HighlightStyle.define([
  { tag: tags.keyword, color: "var(--hone-accent)" },
  { tag: [tags.string, tags.special(tags.string)], color: "var(--hone-ok)" },
  { tag: tags.comment, color: "var(--hone-ink-muted)", fontStyle: "italic" },
  { tag: [tags.number, tags.bool, tags.null], color: "var(--hone-warn)" },
  { tag: [tags.typeName, tags.className], fontWeight: "600" },
  { tag: tags.heading, fontWeight: "700" },
  { tag: tags.emphasis, fontStyle: "italic" },
  { tag: tags.strong, fontWeight: "700" },
  { tag: tags.link, textDecoration: "underline" },
  { tag: [tags.propertyName, tags.attributeName], color: "var(--hone-ink-muted)" },
]);

const theme = EditorView.theme({
  "&": {
    backgroundColor: "var(--hone-bg)",
    color: "var(--hone-ink)",
    fontSize: "13px",
    height: "100%",
  },
  ".cm-scroller": {
    fontFamily: "var(--font-mono)",
    lineHeight: "1.6",
  },
  ".cm-content": { caretColor: "var(--hone-ink)" },
  ".cm-gutters": {
    backgroundColor: "var(--hone-surface)",
    color: "var(--hone-ink-muted)",
    border: "none",
    borderRight: "1px solid var(--hone-line)",
  },
  ".cm-activeLine": { backgroundColor: "transparent" },
  ".cm-activeLineGutter": { backgroundColor: "var(--hone-surface-2)" },
  "&.cm-focused": { outline: "none" },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": {
    backgroundColor: "var(--hone-accent-muted) !important",
  },
  ".hone-target-line": {
    backgroundColor: "var(--hone-accent-muted)",
    // hone's stylesheet removes every box-shadow, so the marker is an outline on the left.
    outline: "none",
    borderLeft: "3px solid var(--hone-accent)",
  },
});

const setTarget = StateEffect.define();
const target = StateField.define({
  create: () => Decoration.none,
  update(lines, transaction) {
    lines = lines.map(transaction.changes);
    for (const effect of transaction.effects) {
      if (effect.is(setTarget)) {
        lines =
          effect.value === null
            ? Decoration.none
            : Decoration.set([
                Decoration.line({ class: "hone-target-line" }).range(effect.value),
              ]);
      }
    }
    return lines;
  },
  provide: (field) => EditorView.decorations.from(field),
});

const views = new Map();

function gotoLine(view, line) {
  const number = Math.min(Math.max(1, line || 1), view.state.doc.lines);
  const position = view.state.doc.line(number).from;
  view.dispatch({
    effects: [
      setTarget.of(line ? position : null),
      EditorView.scrollIntoView(position, { y: "center" }),
    ],
    selection: { anchor: position },
  });
}

window.honeEditor = {
  // Mounts an editor into the element `id`. `onChange` receives the text after every change.
  mount(id, options, onChange) {
    const parent = document.getElementById(id);
    if (!parent) {
      return false;
    }
    views.get(id)?.destroy();
    const language = languages[options.language];
    const view = new EditorView({
      parent,
      state: EditorState.create({
        doc: options.doc || "",
        extensions: [
          basicSetup,
          keymap.of([indentWithTab]),
          theme,
          syntaxHighlighting(highlight),
          target,
          EditorView.lineWrapping,
          EditorState.readOnly.of(Boolean(options.readOnly)),
          EditorView.editable.of(!options.readOnly),
          language ? language() : [],
          EditorView.updateListener.of((update) => {
            if (update.docChanged && onChange) {
              onChange(update.state.doc.toString());
            }
          }),
        ],
      }),
    });
    views.set(id, view);
    if (options.line) {
      gotoLine(view, options.line);
    }
    return true;
  },
  gotoLine(id, line) {
    const view = views.get(id);
    if (view) {
      gotoLine(view, line);
    }
  },
  destroy(id) {
    views.get(id)?.destroy();
    views.delete(id);
  },
};
