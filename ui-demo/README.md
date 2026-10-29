# UI prototype archive

This directory preserves independent UI experiments. It is not the production
InkScope Visualizer, which lives in the sibling `InkScope-Visualizer` repository.

The Vite entry is `src/main.tsx`; its current `App.tsx` is a setup placeholder.
`StageMap.tsx` and `demo-data.ts` contain simulated examples. Root-level `app.ts`
and `style.css` belong to an older static prototype and are not imported by Vite.
None of these examples represents authenticated live gameplay.

Install the pinned dependencies with `bun install --frozen-lockfile` and inspect
`package.json` for the prototype commands. Build output and `node_modules` stay
local and are excluded from version control.
