# Sidebar Row Drag-to-Reorder Implementation (2026-09-04)

## Overview
Removed the dedicated `GripVertical` (`:::`) handle icon button from the left sidebar workspace/worktree items. Now the entire row itself serves as the drag activator for reordering, preserving native click-to-select and individual action button clicks.

## Changes Made

### 1. `ui/src/components/sidebar-dnd/SidebarDragRow.tsx`
- Removed `GripVertical` import from `lucide-react`.
- Removed the separate absolute-positioned `<button ref={setActivatorNodeRef} aria-label={`Reorder ${kind}`}><GripVertical ... /></button>`.
- Removed unnecessary left offset padding (`!project && "pl-4"`).
- Attached `ref={setActivatorNodeRef}`, `{...attributes}`, and `{...listeners}` directly to the row container `div` (`select-none min-w-0`), clearing redundant `role` and `tabIndex` to avoid nested interactive button accessibility conflicts.

### 2. `ui/src/components/Sidebar.tsx`
- In `ProjectHeader`, restored left padding by removing `pl-5` (which was previously reserved for the `size-5` grip button) so the row aligns naturally with `rounded-md pr-1`.
- Added `onPointerDown={(event) => event.stopPropagation()}` on the Chevron accordion toggle button and Add Worktree icon button to ensure clicking these distinct action controls never initiates a row drag gesture.
- Added `onPointerDown={(event) => event.stopPropagation()}` on the expanded worktree list container to ensure worktree dragging never bubbles to the parent project's sortable container.

### 3. `ui/src/components/WorktreeList.tsx`
- Added `onPointerDown={(event) => event.stopPropagation()}` on the Delete Worktree icon button to ensure clicking delete never initiates a row drag.

### 4. `ui/src/components/Sidebar.dnd.test.tsx`
- Added unit test asserting that no separate reorder grip handles (`aria-label="Reorder project"`, `aria-label="Reorder worktree"`) exist in the DOM, and that the workspace rows remain directly draggable and interactive.

## Verification
- `bun run --cwd ui test src/components/Sidebar src/components/WorktreeList`: All 50 tests passing across 4 test suites.
- `bun run --cwd ui build`: Full TypeScript check (`tsc`) and Vite production bundle passed without warnings or errors.
