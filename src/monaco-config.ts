import * as monaco from 'monaco-editor';
import { loader } from '@monaco-editor/react';

// check if BUNDLER is defined and equals "bun"
if (typeof BUNDLER !== "undefined" && BUNDLER === "bun") {
    const editorWorker = new Worker(new URL("./src/monaco/editor.worker.js", import.meta.url), { type: "module" });
    const jsonWorker = new Worker(new URL("./src/monaco/json.worker.js", import.meta.url), { type: "module" });
    const cssWorker = new Worker(new URL("./src/monaco/css.worker.js", import.meta.url), { type: "module" });
    const htmlWorker = new Worker(new URL("./src/monaco/html.worker.js", import.meta.url), { type: "module" });
    const tsWorker = new Worker(new URL("./src/monaco/ts.worker.js", import.meta.url), { type: "module" });
    self.MonacoEnvironment = {
        getWorker(_, label) {
            if (label === 'json') {
                return jsonWorker;
            }
            if (label === 'css' || label === 'scss' || label === 'less') {
                return cssWorker;
            }
            if (label === 'html' || label === 'handlebars' || label === 'razor') {
                return htmlWorker;
            }
            if (label === 'typescript' || label === 'javascript') {
                return tsWorker;
            }
            return editorWorker;
        },
    };
    loader.config({ monaco });
}

export { monaco };
