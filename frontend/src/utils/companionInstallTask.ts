export const COMPANION_INSTALL_TASK_TITLE =
  'Install and integrate Helios Kanban Web Companion';

export const COMPANION_INSTALL_TASK_DESCRIPTION = `Goal: Install and integrate the Helios Kanban Web Companion so it renders at the app root in development. Alt+Click (⌥ Option on macOS) locates the component source; the companion sends open-in-editor to the parent via postMessage so it works inside the Helios Kanban iframe preview.

Packages (pick one — do NOT install vibe-kanban-web-companion):
- React (Next.js, CRA, Vite, or other JSX-source setups): react-helios-kanban-companion
- Vue 2.7 (vite-plugin-vue2 or webpack vue-loader): vue-helios-kanban-companion
Docs: https://github.com/SolomonFang/helios-kanban-web-companion

Do:
1) Detect package manager from lockfiles and use it:
   - pnpm-lock.yaml → pnpm add <package>
   - yarn.lock → yarn add <package>
   - package-lock.json → npm i <package>
   - bun.lockb → bun add <package>
   If already listed in package.json dependencies, skip install.

2) Detect framework and app entry:
   - Next.js (pages router): pages/_app.(tsx|js)
   - Next.js (app router): app/layout.(tsx|js) or an app/providers.(tsx|js)
   - Vite/CRA: src/main.(tsx|jsx|ts|js) and src/App.(tsx|jsx|ts|js)
   - Vue 2: App.vue (or the root component that mounts the app)
   - Monorepo: operate in the correct package for the web app.
   Confirm by reading package.json and directory structure.

3) Integrate HeliosKanbanCompanion at the app root (once):
   React:
     import { HeliosKanbanCompanion } from 'react-helios-kanban-companion';
     - Vite/CRA: render <HeliosKanbanCompanion /> next to <App /> at the root.
     - Next.js (pages): render in pages/_app.*
     - Next.js (app): render in app/layout.* or a client providers component.
     - For Next.js, if SSR issues arise, use dynamic import with ssr: false.
   Vue 2:
     import { HeliosKanbanCompanion } from 'vue-helios-kanban-companion';
     Register the component and render <helios-kanban-companion /> near the root (e.g. App.vue).

4) Verify:
   - Type-check, lint/format if configured.
   - Ensure it compiles and renders without SSR/hydration errors.
   - Both packages are tree-shaken out of production builds; test with the dev server.

Acceptance:
- The correct companion package is installed in the web app package.
- HeliosKanbanCompanion is rendered once at the app root without SSR/hydration errors.
- Build/type-check passes.`;
