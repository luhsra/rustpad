import * as monaco from 'monaco-editor';
import { loader } from '@monaco-editor/react';

let editorWorker: Worker | undefined = undefined;
let jsonWorker: Worker | undefined = undefined;
let cssWorker: Worker | undefined = undefined;
let htmlWorker: Worker | undefined = undefined;
let tsWorker: Worker | undefined = undefined;

self.MonacoEnvironment = {
    getWorker(_, label) {
        if (label === 'json') {
            jsonWorker ??= new Worker("/src/monaco/json.worker.js", { type: "module" });
            return jsonWorker;
        }
        if (label === 'css' || label === 'scss' || label === 'less') {
            cssWorker ??= new Worker("/src/monaco/css.worker.js", { type: "module" });
            return cssWorker;
        }
        if (label === 'html' || label === 'handlebars' || label === 'razor') {
            htmlWorker ??= new Worker("/src/monaco/html.worker.js", { type: "module" });
            return htmlWorker;
        }
        if (label === 'typescript' || label === 'javascript') {
            tsWorker ??= new Worker("/src/monaco/ts.worker.js", { type: "module" });
            return tsWorker;
        }
        editorWorker ??= new Worker("/src/monaco/editor.worker.js", { type: "module" });
        return editorWorker;
    },
};
loader.config({ monaco });

export { monaco };
