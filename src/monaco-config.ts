import * as monaco from 'monaco-editor';
import { loader } from '@monaco-editor/react';

// Set up loader to use bundled monaco
loader.config({ monaco });

// Configure monaco environment for workers
// In development with Bun server, this may fall back to main thread
self.MonacoEnvironment = {
//   getWorker(_: unknown, label: string) {
//     // TODO
//   },
};

export { monaco };
