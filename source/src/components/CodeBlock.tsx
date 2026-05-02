/**
 * CodeBlock — canonical Yeti shared component for read-only and editable code display.
 *
 * Source of truth: ~/yeti/shared-components/CodeBlock.tsx
 * Synced to each app at: source/src/components/CodeBlock.tsx
 *
 * DO NOT EDIT IN-APP. Edit the canonical file and run the sync script:
 *   ~/yeti/shared-components/sync.sh
 *
 * Built on @uiw/react-codemirror (CodeMirror 6). Supports graphql, json, yaml,
 * sql, rust, ts/js, html, css, markdown. Default theme is one-dark with
 * transparent backgrounds + var(--font-size-xs); the surrounding .panel /
 * .panel-content surface shows through.
 */

import { useCallback, useMemo } from "react";
import CodeMirror, {
  EditorView,
  type Extension,
  type ReactCodeMirrorProps,
} from "@uiw/react-codemirror";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { tags as t } from "@lezer/highlight";
import { json } from "@codemirror/lang-json";
import { graphql } from "cm6-graphql";
import { yaml } from "@codemirror/lang-yaml";
import { sql } from "@codemirror/lang-sql";
import { rust } from "@codemirror/lang-rust";
import { javascript } from "@codemirror/lang-javascript";
import { html } from "@codemirror/lang-html";
import { css as cssLang } from "@codemirror/lang-css";
import { markdown } from "@codemirror/lang-markdown";

export type CodeLanguage =
  | "graphql"
  | "gql"
  | "json"
  | "yaml"
  | "yml"
  | "sql"
  | "rust"
  | "rs"
  | "javascript"
  | "js"
  | "jsx"
  | "typescript"
  | "ts"
  | "tsx"
  | "html"
  | "css"
  | "markdown"
  | "md"
  | "text"
  | "plain";

interface CodeBlockProps {
  /**
   * The code content. Either pass `value` directly, or supply the code as
   * children of the component (`<CodeBlock>{`…`}</CodeBlock>`); when both
   * are present, `value` wins.
   */
  value?: string;
  /** Code content as JSX children (string only). Used when `value` is not set. */
  children?: string;
  /**
   * Language id — controls syntax highlighting. If omitted, derived from
   * `label` (e.g. "schema.graphql" → "graphql"); falls back to "json" when
   * neither is set. Pass "text"/"plain" for no highlighting.
   */
  language?: CodeLanguage | string;
  /** Whether the user can edit. When true, you must pass `onChange`. */
  editable?: boolean;
  /** Called with the new value when the user edits. */
  onChange?: (value: string) => void;
  /** Shown when `value` is empty. Renders an `.empty-state`-style hint. */
  placeholder?: string;
  /** Optional file-name pill rendered above the editor. */
  label?: string;
  /** Extra class name on the outer wrapper. */
  className?: string;
  /**
   * Override the editor theme. Defaults to the Yeti hljs palette. Pass a
   * different `Extension` (or array of them) to swap.
   */
  theme?: ReactCodeMirrorProps["theme"];
  /** Pass-through for additional CodeMirror extensions. */
  extensions?: Extension[];
  /** Show the gutter (line numbers, fold markers). Defaults to false. */
  showGutter?: boolean;
}

/**
 * Yeti syntax palette — colors pulled from the per-app yeti _vars.css so the
 * code editor matches the surrounding UI palette. The status colors
 * (success/warning/error/info) double as syntax-token colors:
 *
 *   string              → var(--color-success)   (green)
 *   keyword / built-in  → var(--color-warning)   (orange)
 *   property / field /  → var(--color-info)      (blue) — what was previously
 *   constant / bool /                                    purple lives here now
 *   null / literal
 *   number              → var(--color-info)      (blue, same as constants)
 *   meta / directive    → #b3ae60                (kept yellow — no var match)
 *   typeName / atom /   → var(--color-white)
 *   variable / punct
 *   tagName             → #d5b778                (kept yellow)
 *   heading             → var(--color-success)   (bold)
 *   invalid             → var(--color-error)
 *   comment             → var(--color-grey)      (italic)
 *   docComment          → var(--color-success)   (italic)
 *   regexp              → #42c3d4                (kept cyan)
 *   function            → var(--color-info)
 *   link                → var(--color-info)      (underline)
 */
const yetiHighlightStyle = HighlightStyle.define([
  // Comments
  { tag: [t.comment, t.lineComment, t.blockComment], color: "var(--color-grey)", fontStyle: "italic" },
  { tag: t.docComment, color: "var(--color-success)", fontStyle: "italic" },

  // Keywords + builtins
  { tag: [t.keyword, t.controlKeyword, t.definitionKeyword, t.moduleKeyword, t.operatorKeyword], color: "var(--color-warning)" },
  { tag: t.standard(t.variableName), color: "var(--color-warning)" },

  // Strings + regexp
  { tag: [t.string, t.special(t.string)], color: "var(--color-success)" },
  { tag: t.regexp, color: "#42c3d4" },

  // Numbers
  { tag: [t.number, t.integer, t.float], color: "var(--color-info)" },

  // Constants / booleans / null / field-name properties — what was previously
  // rendered purple (#c77dbb) now uses the palette's blue (--color-info).
  { tag: [t.constant(t.variableName), t.bool, t.null, t.literal, t.propertyName], color: "var(--color-info)" },

  // Functions
  { tag: [t.function(t.variableName), t.function(t.propertyName)], color: "var(--color-info)" },

  // Meta / annotations / macros / modifiers (GraphQL @directives land here).
  { tag: [t.meta, t.annotation, t.macroName, t.modifier], color: "#b3ae60" },

  // Tags (HTML/XML/JSX angle-bracketed names)
  { tag: [t.tagName, t.angleBracket], color: "#d5b778" },

  // Markdown headings stay distinct (success-tinted, bold).
  { tag: t.heading, color: "var(--color-success)", fontWeight: "bold" },

  // Identifiers, type references, atoms, attribute names, punctuation —
  // everything that should render as plain body text.
  {
    tag: [
      t.variableName,
      t.typeName,
      t.className,
      t.namespace,
      t.atom,
      t.attributeName,
      t.attributeValue,
      t.punctuation,
      t.operator,
      t.bracket,
      t.separator,
      t.paren,
      t.brace,
    ],
    color: "var(--color-white)",
  },

  // Inline markdown / docs
  { tag: t.link, color: "var(--color-info)", textDecoration: "underline" },
  { tag: t.strong, fontWeight: "bold" },
  { tag: t.emphasis, fontStyle: "italic" },
  { tag: t.invalid, color: "var(--color-error)" },
]);

const yetiSyntax: Extension = syntaxHighlighting(yetiHighlightStyle);

// `!important` is needed because previously oneDark's scoped class won the
// cascade; we keep `!important` here so an app-level theme override (if any)
// also can't accidentally re-introduce a background.
const TRANSPARENT = "transparent !important";
const FONT_SIZE = "var(--font-size-sm) !important";
const FONT_FAMILY = "var(--font-family-mono) !important";

const transparentTheme: Extension = EditorView.theme({
  "&": {
    backgroundColor: TRANSPARENT,
    color: "inherit",
    fontSize: FONT_SIZE,
    fontFamily: FONT_FAMILY,
    height: "100%",
  },
  ".cm-editor": {
    backgroundColor: TRANSPARENT,
    height: "100%",
  },
  ".cm-scroller": {
    backgroundColor: TRANSPARENT,
    fontFamily: FONT_FAMILY,
    overflowY: "auto",
    overflowX: "auto",
  },
  ".cm-content": {
    backgroundColor: TRANSPARENT,
    fontSize: FONT_SIZE,
    fontFamily: FONT_FAMILY,
  },
  ".cm-line": {
    backgroundColor: TRANSPARENT,
  },
  ".cm-line:hover": {
    backgroundColor: TRANSPARENT,
  },
  ".cm-gutters": {
    backgroundColor: TRANSPARENT,
    border: "none",
  },
  ".cm-gutterElement": {
    backgroundColor: TRANSPARENT,
  },
  ".cm-gutterElement:hover": {
    backgroundColor: TRANSPARENT,
  },
  ".cm-activeLine": {
    backgroundColor: TRANSPARENT,
  },
  ".cm-activeLineGutter": {
    backgroundColor: TRANSPARENT,
  },
  "&.cm-focused": {
    outline: "none",
  },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": {
    backgroundColor: TRANSPARENT,
  },
  ".cm-selectionMatch": {
    backgroundColor: TRANSPARENT,
  },
  ".cm-cursor": {
    borderLeftColor: "var(--color-white)",
  },
});

function languageExtension(lang: string | undefined): Extension[] {
  switch (lang) {
    case "graphql":
    case "gql":
      return [graphql()];
    case "json":
      return [json()];
    case "yaml":
    case "yml":
    case "toml":
      return [yaml()];
    case "sql":
      return [sql()];
    case "rust":
    case "rs":
      return [rust()];
    case "javascript":
    case "js":
      return [javascript({ jsx: true })];
    case "jsx":
      return [javascript({ jsx: true })];
    case "typescript":
    case "ts":
      return [javascript({ jsx: false, typescript: true })];
    case "tsx":
      return [javascript({ jsx: true, typescript: true })];
    case "html":
      return [html()];
    case "css":
      return [cssLang()];
    case "markdown":
    case "md":
      return [markdown()];
    case "text":
    case "plain":
    default:
      return [];
  }
}

/**
 * Derive a language id from a `label` like "schema.graphql" → "graphql",
 * "config.yaml" → "yaml". Used as a fallback when `language` is not passed
 * but `label` is. Returns undefined when nothing matches.
 */
function deriveLanguageFromLabel(label: string | undefined): string | undefined {
  if (!label) return undefined;
  const ext = label.split(".").pop()?.toLowerCase();
  switch (ext) {
    case "graphql":
    case "gql":
    case "json":
    case "yaml":
    case "yml":
    case "toml":
    case "sql":
    case "rs":
    case "rust":
    case "ts":
    case "tsx":
    case "js":
    case "jsx":
    case "html":
    case "css":
    case "md":
    case "markdown":
      return ext;
    case "fiql":
      // FIQL queries — render as plain text (no built-in lezer grammar).
      return "text";
    default:
      // Heuristics matching legacy yeti-www <Code> behavior.
      if (label.includes("schema")) return "graphql";
      if (label.includes("config")) return "yaml";
      return undefined;
  }
}

/**
 * Renders a syntax-highlighted code block. By default read-only and themed
 * with one-dark; toggle `editable` and provide `onChange` for an editor.
 */
export default function CodeBlock({
  value: valueProp,
  children,
  language,
  editable = false,
  onChange,
  placeholder,
  label,
  className,
  theme,
  extensions,
  showGutter = false,
}: CodeBlockProps) {
  const value = valueProp ?? children ?? "";
  const handleChange = useCallback(
    (v: string) => {
      if (editable) onChange?.(v);
    },
    [editable, onChange],
  );

  const resolvedLanguage = language ?? deriveLanguageFromLabel(label) ?? "json";

  const allExtensions = useMemo(() => {
    const lang = languageExtension(resolvedLanguage);
    // Order: language → yeti syntax palette → transparent overrides → user.
    // Long lines scroll horizontally (no wrap).
    return [...lang, yetiSyntax, transparentTheme, ...(extensions ?? [])];
  }, [resolvedLanguage, extensions]);

  // Empty + placeholder branch — keeps the visual chrome (label + outer
  // wrapper) but renders the placeholder hint instead of an empty editor.
  if (!value && placeholder) {
    return (
      <div className={["code-block", className].filter(Boolean).join(" ")}>
        {label ? <span className="code-label">{label}</span> : null}
        <div className="code-block-empty">
          <span className="empty-hint">{placeholder}</span>
        </div>
      </div>
    );
  }

  return (
    <div className={["code-block", className].filter(Boolean).join(" ")}>
      {label ? <span className="code-label">{label}</span> : null}
      <CodeMirror
        value={value}
        editable={editable}
        readOnly={!editable}
        theme={theme}
        extensions={allExtensions}
        onChange={handleChange}
        basicSetup={{
          lineNumbers: showGutter,
          foldGutter: showGutter,
          highlightActiveLine: false,
          highlightActiveLineGutter: false,
          highlightSelectionMatches: false,
          dropCursor: editable,
          allowMultipleSelections: editable,
          autocompletion: editable,
          searchKeymap: editable,
        }}
      />
    </div>
  );
}
